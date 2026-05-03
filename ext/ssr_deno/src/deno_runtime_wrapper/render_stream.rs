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
    let script = format!(
        r#"
        if (typeof globalThis.__SSR_STREAM_SENTINEL === 'undefined') {{
            globalThis.__SSR_STREAM_SENTINEL = {{}};
        }}
        globalThis.__ssr_stream_result = globalThis.__SSR_STREAM_SENTINEL;
        var __bundle = globalThis.__ssr_bundles[{bundle_id_json:?}];
        if (!__bundle || typeof __bundle.render !== 'function') {{
            throw new Error('Bundle not found: {bundle_id_json:?}');
        }}
        var __result = __bundle.render({args_json});
        if (__result && typeof __result.then === 'function') {{
            __result.then(
                (html) => {{ globalThis.__ssr_stream_result = html; }},
                (err) => {{ globalThis.__ssr_stream_result = 'ERROR:' + (err.message || String(err)); }}
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

        // Check if the render has completed.
        if let Some(result) = read_stream_result(worker) {
            return Ok(result);
        }
    }
}

/// Checks `__ssr_stream_result` for the final rendered HTML.
/// Uses a sentinel object (`__SSR_STREAM_SENTINEL`) to distinguish
/// "not yet set" from a render that returned null/undefined.
fn read_stream_result(worker: &mut MainWorker) -> Option<String> {
    // Check sentinel: compare __ssr_stream_result against the unique sentinel
    // by checking a boolean flag that's set when the stream completes.
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
