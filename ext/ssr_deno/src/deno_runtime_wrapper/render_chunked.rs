use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use deno_runtime::deno_core::v8;
use deno_runtime::worker::MainWorker;
use tokio::sync::mpsc;

use super::SSRDenoError;
use super::render::{RenderState, cleanup_render_globals, poll_render_state};
use super::watchdog::Watchdog;

// ---------------------------------------------------------------------------
// Chunked render — chunks flow through JS global array to Ruby
// ---------------------------------------------------------------------------

/// Runs a render where JS pushes chunks to `globalThis.__ssr_chunks`
/// (a plain array). Each event-loop tick, Rust drains the array and sends
/// chunks through `chunk_tx` to the Ruby consumer.
///
/// This poll-based design avoids the need to expose `Deno.core.ops` to user
/// scripts (which is hidden post-bootstrap in deno_runtime 0.255+). For SSR
/// workloads (bounded HTML fragments), the absence of async backpressure is
/// acceptable -- chunks are small and the Ruby consumer is fast.
///
/// JS API: `globalThis.__ssr_push_chunk(string)` -- synchronous.
///
/// Completion protocol:
/// - Success: all chunks drained, promise resolves -> `Ok(())`
/// - Error (JS reject): `Err(SSRDenoError::Render(msg))`
/// - Error (timeout): `Err(SSRDenoError::Render("..."))`
/// - Error (OOM): `Err(SSRDenoError::OutOfMemory("..."))`
///
/// `chunk_tx` is dropped when the function returns, causing the receiver to
/// get `None` on the next `recv()`.
pub async fn render_chunked(
    worker: &mut MainWorker,
    bundle_id: &str,
    args_json: &str,
    render_timeout_ms: u64,
    chunk_tx: mpsc::Sender<String>,
    oom_triggered: &AtomicBool,
) -> Result<(), SSRDenoError> {
    let bundle_id_js = serde_json::to_string(bundle_id)
        .unwrap_or_else(|_| format!("\"{}\"", bundle_id));
    let args_json_js = serde_json::to_string(args_json)
        .unwrap_or_else(|_| format!("\"{}\"", args_json));

    // Set up the chunk array + push function, then kick off the render.
    let script = format!(
        r#"
        if (typeof globalThis.__SSR_DENO_SENTINEL === 'undefined') {{
            globalThis.__SSR_DENO_SENTINEL = {{}};
        }}
        globalThis.__ssr_deno_result = globalThis.__SSR_DENO_SENTINEL;
        globalThis.__ssr_deno_error = null;
        globalThis.__ssr_chunks = [];
        globalThis.__ssr_push_chunk = function(chunk) {{
            globalThis.__ssr_chunks.push(chunk);
        }};
        var __bundle = globalThis.__ssr_bundles[{bundle_id_js}];
        if (!__bundle || typeof __bundle.render !== 'function') {{
            throw new Error('Bundle not found: ' + {bundle_id_js});
        }}
        var __result = __bundle.render({args_json_js});
        if (__result && typeof __result.then === 'function') {{
            __result.then(
                (html) => {{ globalThis.__ssr_deno_result = html; }},
                (err) => {{ globalThis.__ssr_deno_error = (err && err.message) || String(err); }}
            );
        }} else {{
            globalThis.__ssr_deno_result = __result;
        }}
        "#,
        bundle_id_js = bundle_id_js,
        args_json_js = args_json_js,
    );
    // Arm the watchdog before execute_script — covers sync-blocking renders.
    let v8_handle = worker.js_runtime.v8_isolate().thread_safe_handle();
    let timeout_triggered = Arc::new(AtomicBool::new(false));
    let watchdog = Watchdog::spawn(v8_handle, render_timeout_ms, timeout_triggered.clone());

    let exec_result = worker
        .execute_script("<ssr-deno:render-chunked-start>", script.into());

    if let Err(e) = exec_result {
        watchdog.cancel();
        cleanup_render_globals(worker);

        if oom_triggered.load(Ordering::SeqCst) {
            worker.js_runtime.v8_isolate().cancel_terminate_execution();
            return Err(SSRDenoError::OutOfMemory(
                "JS heap out of memory - the isolate reached its configured heap limit".into(),
            ));
        }
        if timeout_triggered.load(Ordering::SeqCst) {
            worker.js_runtime.v8_isolate().cancel_terminate_execution();
            return Err(SSRDenoError::Render("Chunked render timed out".into()));
        }

        let msg = e.to_string();
        return if msg.contains("Bundle not found:") {
            Err(SSRDenoError::BundleNotFound(msg))
        } else {
            Err(SSRDenoError::Render(format!("Chunked render failed to start: {msg}")))
        };
    }

    // Run the event loop -- each tick, drain __ssr_chunks and forward to Ruby.
    // The watchdog is the sole timeout authority — no separate deadline check.
    let result = loop {
        let _ = worker.run_up_to_duration(Duration::from_millis(50)).await;

        if oom_triggered.load(Ordering::SeqCst) {
            worker.js_runtime.v8_isolate().cancel_terminate_execution();
            break Err(SSRDenoError::OutOfMemory(
                "JS heap out of memory - the isolate reached its configured heap limit".into(),
            ));
        }
        if timeout_triggered.load(Ordering::SeqCst) {
            worker.js_runtime.v8_isolate().cancel_terminate_execution();
            break Err(SSRDenoError::Render("Chunked render timed out".into()));
        }

        // Drain pending chunks from the JS array.
        drain_chunks(worker, &chunk_tx).await;

        match poll_render_state(worker) {
            RenderState::Pending => continue,
            RenderState::Error(msg) => break Err(SSRDenoError::Render(msg)),
            RenderState::Done(_) => {
                // Final drain -- the last event-loop tick may have produced
                // chunks that are sitting in __ssr_chunks after the promise
                // resolved.
                drain_chunks(worker, &chunk_tx).await;
                break Ok(());
            }
        }
    };

    watchdog.cancel();

    // If the watchdog or OOM callback fired between the loop's deadline check
    // and watchdog.cancel(), the isolate has pending termination. Clear it so
    // the isolate is reusable for future operations.
    if timeout_triggered.load(Ordering::SeqCst) || oom_triggered.load(Ordering::SeqCst) {
        worker.js_runtime.v8_isolate().cancel_terminate_execution();
    }

    // Clean up JS globals to avoid leaking state across renders.
    let _ = worker.execute_script(
        "<ssr-deno:render-chunked-cleanup>",
        "globalThis.__ssr_chunks = undefined; globalThis.__ssr_push_chunk = undefined;"
            .to_string().into(),
    );

    // Drop chunk_tx (moved into this function) -- closes the channel so the
    // Ruby consumer's `blocking_recv()` returns `None`, signaling EOS.
    drop(chunk_tx);

    result
}

/// Drains `globalThis.__ssr_chunks` and sends each element through `chunk_tx`.
/// Uses a single `execute_script` call that returns a JSON array of the
/// pending chunks, then clears the JS array.
async fn drain_chunks(worker: &mut MainWorker, chunk_tx: &mpsc::Sender<String>) {
    let Ok(global_val) = worker.execute_script(
        "<ssr-deno:render-drain>",
        "(function() { var c = globalThis.__ssr_chunks; globalThis.__ssr_chunks = []; return c && c.length > 0 ? JSON.stringify(c) : null; })()"
            .to_string().into(),
    ) else {
        return;
    };

    let context = worker.js_runtime.main_context();
    let isolate = worker.js_runtime.v8_isolate();
    let mut scope_storage = std::pin::pin!(v8::HandleScope::new(isolate));
    let mut scope = scope_storage.as_mut().init();
    let context_local = v8::Local::new(&mut scope, &context);
    let mut context_scope = v8::ContextScope::new(&mut scope, context_local);

    let local_val = v8::Local::new(&mut context_scope, &global_val);

    if local_val.is_null_or_undefined() {
        return;
    }

    let json_str = local_val.to_rust_string_lossy(&mut context_scope);

    // Drop the scope before sending -- v8 handles can't cross await points.
    drop(context_scope);
    drop(scope);

    // Parse the JSON array of chunk strings.
    if let Ok(chunks) = serde_json::from_str::<Vec<String>>(&json_str) {
        for chunk in chunks {
            if chunk_tx.send(chunk).await.is_err() {
                // Consumer disconnected (Ruby block raised or was interrupted).
                // Stop sending — the render promise will settle normally and
                // completion/error is communicated via reply_rx in lib.rs.
                break;
            }
        }
    }
}
