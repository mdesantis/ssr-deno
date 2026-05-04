use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use deno_runtime::worker::MainWorker;
use tokio::sync::mpsc;

use super::SSRDenoError;
use super::render_stream::{StreamState, poll_stream_state};

// ---------------------------------------------------------------------------
// Op-based chunked streaming render — chunks flow through async op to Ruby
// ---------------------------------------------------------------------------

/// Runs a streaming render where JS pushes chunks via the async
/// `globalThis.__ssr_push_chunk_op(string)` function (captured during bootstrap
/// from `Deno.core.ops.op_ssr_push_chunk`). Each call goes through the Deno op
/// system, which `send().await`s into the `mpsc::Sender<String>` stored in
/// `OpState`. This provides true end-to-end backpressure: if the channel buffer
/// (64 slots) is full, the JS `await` suspends until the Ruby consumer drains.
///
/// JS API: `await globalThis.__ssr_push_chunk_op(string)` — async.
///
/// Completion protocol:
/// - Success: promise resolves → `Ok(())`
/// - Error (JS reject): `Err(SSRDenoError::Render(msg))`
/// - Error (timeout): `Err(SSRDenoError::Render("..."))`
/// - Error (OOM): `Err(SSRDenoError::OutOfMemory("..."))`
///
/// `chunk_tx` is removed from OpState and dropped when the function returns,
/// causing the receiver to get `None` on the next `recv()`.
pub async fn render_streaming_chunked_op(
    worker: &mut MainWorker,
    bundle_id: &str,
    args_json: &str,
    render_timeout_ms: u64,
    chunk_tx: mpsc::Sender<String>,
    oom_triggered: &AtomicBool,
) -> Result<(), SSRDenoError> {
    // Register chunk_tx in OpState so op_ssr_push_chunk can find it.
    worker.js_runtime.op_state().borrow_mut().put(chunk_tx);

    let bundle_id_js = serde_json::to_string(bundle_id)
        .unwrap_or_else(|_| format!("\"{}\"", bundle_id));

    // Kick off the render. The JS bundle calls
    // `await globalThis.__ssr_push_chunk_op(chunk)` for each fragment.
    // The op delivers each chunk directly to the mpsc channel.
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
        .execute_script("<ssr-deno:stream-chunked-op-start>", script.into())
        .map_err(|e| SSRDenoError::Render(format!("Op-based chunked streaming render failed to start: {e}")))?;

    // Run the event loop — the async op handles chunk delivery automatically.
    // We just need to pump the event loop and check for completion/error.
    let deadline = Instant::now() + Duration::from_millis(render_timeout_ms);

    let result = loop {
        if Instant::now() >= deadline {
            if oom_triggered.load(Ordering::SeqCst) {
                break Err(SSRDenoError::OutOfMemory(
                    "JS heap out of memory — the isolate reached its configured heap limit".into(),
                ));
            }
            break Err(SSRDenoError::Render("Op-based chunked streaming render timed out".into()));
        }

        let remaining = deadline.saturating_duration_since(Instant::now());
        let tick = std::cmp::min(remaining, Duration::from_millis(50));
        let _ = worker.run_up_to_duration(tick).await;

        if oom_triggered.load(Ordering::SeqCst) {
            break Err(SSRDenoError::OutOfMemory(
                "JS heap out of memory — the isolate reached its configured heap limit".into(),
            ));
        }

        // No drain_chunks needed — the op sends chunks directly to the channel.
        // Just check if the render promise has settled.
        match poll_stream_state(worker) {
            StreamState::Pending => continue,
            StreamState::Error(msg) => break Err(SSRDenoError::Render(msg)),
            StreamState::Done(_) => break Ok(()),
        }
    };

    // Remove the sender from OpState to avoid stale state on isolate reuse.
    // This also drops chunk_tx, closing the channel → Ruby sees None.
    drop(worker.js_runtime.op_state().borrow_mut().take::<mpsc::Sender<String>>());

    result
}
