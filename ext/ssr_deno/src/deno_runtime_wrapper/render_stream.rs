use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use deno_core::{op2, OpState};
use deno_core::error::CoreError;
use deno_runtime::deno_core::v8;
use deno_runtime::worker::MainWorker;
use tokio::sync::mpsc;

use super::DenoError;

// ---------------------------------------------------------------------------
// Op: receive a chunk of HTML from JS during streaming render
// ---------------------------------------------------------------------------

#[op2(fast)]
pub fn op_ssr_push_chunk(#[string] chunk: String, state: &mut OpState) -> Result<(), CoreError> {
    let tx = state.borrow::<mpsc::Sender<String>>();
    // TODO: When true streaming (chunked HTTP response) is wired up, replace
    // try_send with send().await for backpressure. Currently chunks are unused
    // — only the final __ssr_stream_result matters. Silent drop is intentional.
    let _ = tx.try_send(chunk);
    Ok(())
}

// ---------------------------------------------------------------------------
// Render streaming
// ---------------------------------------------------------------------------

pub async fn render_streaming(
    worker: &mut MainWorker,
    bundle_id: &str,
    args_json: &str,
    render_timeout_ms: u64,
    chunk_tx: mpsc::Sender<String>,
    oom_triggered: &AtomicBool,
) -> Result<String, DenoError> {
    // Register chunk_tx in OpState so op_ssr_push_chunk can find it
    worker.js_runtime.op_state().borrow_mut().put(chunk_tx);

    // Kick off the render. The bundle's render function is stored at
    // globalThis.__ssr_bundles[bundle_id].render. It returns a Promise
    // that resolves with the final HTML when streaming completes.
    //
    // Error handling: rejected promises store the error message in a
    // separate `__ssr_stream_error` global (not in `__ssr_stream_result`).
    // The poll loop checks `__ssr_stream_error` first and returns
    // `DenoError::Render` when set, ensuring proper exception propagation
    // back to Ruby.
    let script = format!(
        r#"
        if (typeof globalThis.__SSR_STREAM_SENTINEL === 'undefined') {{
            globalThis.__SSR_STREAM_SENTINEL = {{}};
        }}
        globalThis.__ssr_stream_result = globalThis.__SSR_STREAM_SENTINEL;
        globalThis.__ssr_stream_error = null;
        var __bundle = globalThis.__ssr_bundles[{bundle_id_json:?}];
        if (!__bundle || typeof __bundle.render !== 'function') {{
            throw new Error('Bundle not found: {bundle_id_json:?}');
        }}
        var __result = __bundle.render({args_json});
        if (__result && typeof __result.then === 'function') {{
            __result.then(
                (html) => {{ globalThis.__ssr_stream_result = html; }},
                (err) => {{ globalThis.__ssr_stream_error = (err && err.message) || String(err); }}
            );
        }} else {{
            globalThis.__ssr_stream_result = __result;
        }}
        "#,
        bundle_id_json = bundle_id,
        args_json = args_json,
    );
    worker
        .execute_script("<ssr-deno:stream-start>", script.into())
        .map_err(|e| DenoError::Render(format!("Streaming render failed to start: {e}")))?;

    // Run the event loop until the render completes or the timeout expires.
    let deadline = Instant::now() + Duration::from_millis(render_timeout_ms);

    loop {
        if Instant::now() >= deadline {
            if oom_triggered.load(Ordering::SeqCst) {
                return Err(DenoError::OutOfMemory(
                    "JS heap out of memory — the isolate reached its configured heap limit".into(),
                ));
            }
            return Err(DenoError::Render("Streaming render timed out".into()));
        }

        // Run the event loop briefly to let the stream progress.
        let remaining = deadline - Instant::now();
        let tick = std::cmp::min(remaining, Duration::from_millis(50));
        let _ = worker.run_up_to_duration(tick).await;

        if oom_triggered.load(Ordering::SeqCst) {
            return Err(DenoError::OutOfMemory(
                "JS heap out of memory — the isolate reached its configured heap limit".into(),
            ));
        }

        // Check if the render rejected with an error.
        if let Some(err_msg) = read_stream_error(worker) {
            return Err(DenoError::Render(err_msg));
        }

        // Check if the render has completed successfully.
        if let Some(result) = read_stream_result(worker) {
            return Ok(result);
        }
    }
}

/// Checks `globalThis.__ssr_stream_error` for a rejection message.
/// Returns `Some(msg)` when the streaming render's promise rejected.
fn read_stream_error(worker: &mut MainWorker) -> Option<String> {
    let global_val = worker
        .execute_script(
            "<ssr-deno:stream-error-check>",
            "globalThis.__ssr_stream_error"
                .to_string()
                .into(),
        )
        .ok()?;

    let context = worker.js_runtime.main_context();
    let isolate = worker.js_runtime.v8_isolate();
    let mut scope_storage = std::pin::pin!(v8::HandleScope::new(isolate));
    let mut scope = scope_storage.as_mut().init();
    let context_local = v8::Local::new(&mut scope, &context);
    let mut context_scope = v8::ContextScope::new(&mut scope, context_local);

    let local_val = v8::Local::new(&mut context_scope, &global_val);

    if local_val.is_null_or_undefined() {
        return None;
    }

    Some(local_val.to_rust_string_lossy(&mut context_scope))
}

/// Checks `__ssr_stream_result` for the final rendered HTML.
/// Uses a sentinel object (`__SSR_STREAM_SENTINEL`) to distinguish
/// "not yet set" from a render that returned null/undefined.
fn read_stream_result(worker: &mut MainWorker) -> Option<String> {
    let global_val = worker
        .execute_script(
            "<ssr-deno:stream-check>",
            "globalThis.__ssr_stream_result === globalThis.__SSR_STREAM_SENTINEL \
             ? 'null' \
             : JSON.stringify(globalThis.__ssr_stream_result)"
                .to_string()
                .into(),
        )
        .ok()?;

    let context = worker.js_runtime.main_context();
    let isolate = worker.js_runtime.v8_isolate();
    let mut scope_storage = std::pin::pin!(v8::HandleScope::new(isolate));
    let mut scope = scope_storage.as_mut().init();
    let context_local = v8::Local::new(&mut scope, &context);
    let mut context_scope = v8::ContextScope::new(&mut scope, context_local);

    let local_val = v8::Local::new(&mut context_scope, &global_val);
    let result_str = local_val.to_rust_string_lossy(&mut context_scope);

    if result_str == "null" {
        return None;
    }
    Some(result_str)
}
