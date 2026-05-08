use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use deno_runtime::deno_core::url::Url;
use tokio::runtime;
use tokio::task::LocalSet;

use super::builder::build_worker;
use super::heap_stats::collect_heap_stats;
use super::render;
use super::render_chunked;
use super::types::WorkerMsg;

// ---------------------------------------------------------------------------
// Worker thread (per-isolate)
// ---------------------------------------------------------------------------

pub fn worker_thread_main(
    mut rx: tokio::sync::mpsc::Receiver<WorkerMsg>,
    init_tx: mpsc::SyncSender<Result<(), String>>,
    max_heap_size_mb: usize,
    node_builtins: bool,
) {
    let rt = match runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            let _ = init_tx.send(Err(format!("Failed to build Tokio runtime: {e}")));
            return;
        }
    };

    // LocalSet is required by deno_unsync::spawn_local, which Deno's Web API
    // extensions (e.g. MessagePort used by React 19's scheduler) call internally.
    LocalSet::new().block_on(&rt, async move {
        // Synthetic URL — only required as metadata for MainWorker bootstrap.
        // All bundles are loaded via execute_script, not ES module resolution.
        let main_module_url = match Url::parse("https://ssr-deno.local/") {
            Ok(url) => url,
            Err(e) => {
                let _ = init_tx.send(Err(format!("Cannot build worker URL: {e}")));
                return;
            }
        };

        let oom_triggered = Arc::new(AtomicBool::new(false));

        let mut worker = match build_worker(
            &main_module_url,
            max_heap_size_mb,
            node_builtins,
            oom_triggered.clone(),
        ) {
            Ok(w) => w,
            Err(e) => {
                let _ = init_tx.send(Err(e));
                return;
            }
        };

        let _ = init_tx.send(Ok(()));

        while let Some(msg) = rx.recv().await {
            match msg {
                WorkerMsg::LoadBundle {
                    bundle_id,
                    bundle_path,
                    bundle_code,
                    script_name,
                    reply,
                } => {
                    let result = load_bundle_in_worker(
                        &mut worker,
                        &bundle_id,
                        &bundle_path,
                        bundle_code,
                        script_name,
                        node_builtins,
                    );
                    let _ = reply.send(result);
                }
                WorkerMsg::Render {
                    bundle_id,
                    args_json,
                    render_timeout_ms,
                    reply,
                } => {
                    let result = render::render(
                        &mut worker,
                        &bundle_id,
                        &args_json,
                        render_timeout_ms,
                        &oom_triggered,
                    )
                    .await;
                    let _ = reply.send(result);
                }
                WorkerMsg::RenderChunked {
                    bundle_id,
                    args_json,
                    render_timeout_ms,
                    chunk_tx,
                    reply,
                } => {
                    let result = render_chunked::render_chunked(
                        &mut worker,
                        &bundle_id,
                        &args_json,
                        render_timeout_ms,
                        chunk_tx,
                        &oom_triggered,
                    )
                    .await;
                    let _ = reply.send(result);
                }
                WorkerMsg::HeapStats { reply } => {
                    let result = collect_heap_stats(&mut worker);
                    let _ = reply.send(result);
                }
            }
        }
    });
}

/// Injects `globalThis.require` into the V8 context by loading
/// `createRequire` from Deno's built-in `node:module` via async import.
fn setup_require(worker: &mut deno_runtime::worker::MainWorker) -> Result<(), String> {
    // Idempotency guard: skip the async import + microtask polling when
    // `globalThis.require` is already set from a prior bundle load into
    // the same isolate. Saves ~10ms per subsequent bundle load.
    let check_val = worker
        .execute_script(
            "<ssr-deno:require-guard>",
            "typeof globalThis.require !== 'undefined'"
                .to_string()
                .into(),
        )
        .map_err(|e| format!("Failed to check require: {e}"))?;
    let isolate = worker.js_runtime.v8_isolate();
    let check_ref = check_val.open(isolate);
    if check_ref.is_true() {
        return Ok(());
    }

    // The deno_node extension registers node:module polyfill via its extension
    // system. When import('node:module') is called, the extension serves the
    // source code directly (not through the module loader). We use microtask
    // polling to let the async import resolve synchronously.
    worker
        .execute_script(
            "<ssr-deno:require>",
            r#"
            (async () => {
                const { createRequire } = await import('node:module');
                globalThis.require = createRequire('file:///');
            })();
            "#
            .to_string()
            .into(),
        )
        .map_err(|e| format!("Failed to start require import: {e}"))?;

    let isolate = worker.js_runtime.v8_isolate();
    let deadline = Instant::now() + Duration::from_millis(100);
    // Poll microtasks until the require promise settles or we hit the safety cap.
    // The import targets a built-in extension (node:module) — normally resolves
    // in <1ms, but we allow up to 100ms for heavily loaded systems.
    //
    // No active timeout watchdog — a hung import could block the worker forever.
    // This is acceptable because the import target is a local built-in extension
    // (not network I/O); if it hangs, the entire V8 isolate is already broken.
    // See archived plans/require-backoff.md for exponential backoff analysis
    // (closed: low priority, not worth the churn for exceptional-case safety).
    loop {
        isolate.perform_microtask_checkpoint();
        if Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_micros(50));
    }

    worker
        .execute_script(
            "<ssr-deno:require-verify>",
            r#"
            if (typeof globalThis.require === 'undefined') {
                throw new Error('createRequire failed - globalThis.require is undefined');
            }
            "#
            .to_string()
            .into(),
        )
        .map(|_| ())
        .map_err(|e| format!("setup_require failed: {e}"))
}

/// Evaluates the bundle code and moves `globalThis.render` into the bundle
/// namespace: `globalThis.__ssr_bundles[bundle_id] = { render: globalThis.render }`.
fn load_bundle_in_worker(
    worker: &mut deno_runtime::worker::MainWorker,
    bundle_id: &str,
    _bundle_path: &str,
    bundle_code: Arc<str>,
    script_name: &'static str,
    node_builtins: bool,
) -> Result<(), String> {
    if node_builtins {
        if let Err(e) = setup_require(worker) {
            return Err(format!("Failed to set up require: {e}"));
        }
    }

    if let Err(e) = worker.execute_script(script_name, bundle_code.into()) {
        return Err(format!("Failed to evaluate SSR bundle: {e}"));
    }

    // Always register/overwrite the bundle in __ssr_bundles to support auto-reload.
    // serde_json::to_string produces a guaranteed-valid JS string literal.
    let bundle_id_js =
        serde_json::to_string(bundle_id).expect("serde_json::to_string cannot fail for &str");

    let namespace_script = format!(
        r#"(function(id) {{
            if (typeof globalThis.__ssr_bundles === 'undefined') {{
                globalThis.__ssr_bundles = {{}};
            }}
            if (typeof globalThis.render !== 'function') {{
                throw new Error('Bundle did not assign a function to globalThis.render');
            }}
            globalThis.__ssr_bundles[id] = {{ render: globalThis.render }};
            globalThis.render = undefined;
        }})({bundle_id_js});"#
    );

    worker
        .execute_script("<ssr-deno:namespace>", namespace_script.into())
        .map(|_| ())
        .map_err(|e| format!("Failed to namespace bundle '{bundle_id}': {e}"))
}
