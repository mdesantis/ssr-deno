use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use deno_core::{op2, OpState};
use deno_core::error::CoreError;
use deno_runtime::deno_core::v8;
use deno_runtime::worker::MainWorker;
use tokio::sync::mpsc;

use super::SSRDenoError;

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
) -> Result<String, SSRDenoError> {
    // Register chunk_tx in OpState so op_ssr_push_chunk can find it
    worker.js_runtime.op_state().borrow_mut().put(chunk_tx);

    // Use serde_json for bundle_id injection — produces a guaranteed-valid JS
    // string literal regardless of special characters in the filename.
    let bundle_id_js = serde_json::to_string(bundle_id)
        .unwrap_or_else(|_| format!("\"{}\"", bundle_id));

    // Kick off the render. The bundle's render function is stored at
    // globalThis.__ssr_bundles[bundle_id].render. It returns a Promise
    // that resolves with the final HTML when streaming completes.
    //
    // Error handling: rejected promises store the error message in a
    // separate `__ssr_stream_error` global (not in `__ssr_stream_result`).
    // The poll loop checks `__ssr_stream_error` first and returns
    // `SSRDenoError::Render` when set, ensuring proper exception propagation
    // back to Ruby.
    let script = format!(
        r#"
        if (typeof globalThis.__SSR_STREAM_SENTINEL === 'undefined') {{
            globalThis.__SSR_STREAM_SENTINEL = {{}};
        }}
        globalThis.__ssr_stream_result = globalThis.__SSR_STREAM_SENTINEL;
        globalThis.__ssr_stream_error = null;
        var __bundle = globalThis.__ssr_bundles[{bundle_id_js}];
        if (!__bundle || typeof __bundle.render !== 'function') {{
            throw new Error('Bundle not found: ' + {bundle_id_js});
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
        bundle_id_js = bundle_id_js,
        args_json = args_json,
    );
    worker
        .execute_script("<ssr-deno:stream-start>", script.into())
        .map_err(|e| SSRDenoError::Render(format!("Streaming render failed to start: {e}")))?;

    // Run the event loop until the render completes or the timeout expires.
    let deadline = Instant::now() + Duration::from_millis(render_timeout_ms);

    loop {
        if Instant::now() >= deadline {
            if oom_triggered.load(Ordering::SeqCst) {
                return Err(SSRDenoError::OutOfMemory(
                    "JS heap out of memory — the isolate reached its configured heap limit".into(),
                ));
            }
            return Err(SSRDenoError::Render("Streaming render timed out".into()));
        }

        // Run the event loop briefly to let the stream progress.
        let remaining = deadline.saturating_duration_since(Instant::now());
        let tick = std::cmp::min(remaining, Duration::from_millis(50));
        let _ = worker.run_up_to_duration(tick).await;

        if oom_triggered.load(Ordering::SeqCst) {
            return Err(SSRDenoError::OutOfMemory(
                "JS heap out of memory — the isolate reached its configured heap limit".into(),
            ));
        }

        // Single script call per tick: checks error first, then result.
        // Returns null when still pending, "E:<msg>" on error, or
        // "R:<json>" when the render completed successfully.
        match poll_stream_state(worker) {
            StreamState::Pending => continue,
            StreamState::Error(msg) => return Err(SSRDenoError::Render(msg)),
            StreamState::Done(result) => return Ok(result),
        }
    }
}

// ---------------------------------------------------------------------------
// Stream state polling — single execute_script per tick
// ---------------------------------------------------------------------------

enum StreamState {
    Pending,
    Error(String),
    Done(String),
}

/// Checks both `__ssr_stream_error` and `__ssr_stream_result` in a single
/// `execute_script` call. Returns the stream state as a tagged string:
/// - `null` / undefined → still pending
/// - starts with `E:` → promise rejected, rest is the error message
/// - starts with `R:` → render complete, rest is the JSON result
fn poll_stream_state(worker: &mut MainWorker) -> StreamState {
    let Ok(global_val) = worker.execute_script(
        "<ssr-deno:stream-poll>",
        "globalThis.__ssr_stream_error \
         ? ('E:' + globalThis.__ssr_stream_error) \
         : (globalThis.__ssr_stream_result === globalThis.__SSR_STREAM_SENTINEL \
            ? null \
            : ('R:' + JSON.stringify(globalThis.__ssr_stream_result)))"
            .to_string()
            .into(),
    ) else {
        // Script execution failed — treat as pending (next tick will retry
        // or the timeout will fire).
        return StreamState::Pending;
    };

    let context = worker.js_runtime.main_context();
    let isolate = worker.js_runtime.v8_isolate();
    let mut scope_storage = std::pin::pin!(v8::HandleScope::new(isolate));
    let mut scope = scope_storage.as_mut().init();
    let context_local = v8::Local::new(&mut scope, &context);
    let mut context_scope = v8::ContextScope::new(&mut scope, context_local);

    let local_val = v8::Local::new(&mut context_scope, &global_val);

    if local_val.is_null_or_undefined() {
        return StreamState::Pending;
    }

    let s = local_val.to_rust_string_lossy(&mut context_scope);

    if let Some(err_msg) = s.strip_prefix("E:") {
        StreamState::Error(err_msg.to_string())
    } else if let Some(result) = s.strip_prefix("R:") {
        StreamState::Done(result.to_string())
    } else {
        // Unexpected format — shouldn't happen, but treat as pending.
        StreamState::Pending
    }
}
