use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use deno_runtime::deno_core::v8;
use deno_runtime::worker::MainWorker;
use tokio::sync::mpsc;

use super::SSRDenoError;
use super::render_stream::{StreamState, poll_stream_state};

// ---------------------------------------------------------------------------
// Chunked streaming render — chunks flow through JS global array to Ruby
// ---------------------------------------------------------------------------

/// Runs a streaming render where JS pushes chunks to `globalThis.__ssr_chunks`
/// (a plain array). Each event-loop tick, Rust drains the array and sends
/// chunks through `chunk_tx` to the Ruby consumer.
///
/// This poll-based design avoids the need to expose `Deno.core.ops` to user
/// scripts (which is hidden post-bootstrap in deno_runtime 0.255+). For SSR
/// workloads (bounded HTML fragments), the absence of async backpressure is
/// acceptable — chunks are small and the Ruby consumer is fast.
///
/// JS API: `globalThis.__ssr_push_chunk(string)` — synchronous.
///
/// Completion protocol:
/// - Success: all chunks drained, promise resolves → `Ok(())`
/// - Error (JS reject): `Err(SSRDenoError::Render(msg))`
/// - Error (timeout): `Err(SSRDenoError::Render("..."))`
/// - Error (OOM): `Err(SSRDenoError::OutOfMemory("..."))`
///
/// `chunk_tx` is dropped when the function returns, causing the receiver to
/// get `None` on the next `recv()`.
pub async fn render_streaming_chunked(
    worker: &mut MainWorker,
    bundle_id: &str,
    args_json: &str,
    render_timeout_ms: u64,
    chunk_tx: mpsc::Sender<String>,
    oom_triggered: &AtomicBool,
) -> Result<(), SSRDenoError> {
    let bundle_id_js = serde_json::to_string(bundle_id)
        .unwrap_or_else(|_| format!("\"{}\"", bundle_id));

    // Set up the chunk array + push function, then kick off the render.
    let script = format!(
        r#"
        if (typeof globalThis.__SSR_STREAM_SENTINEL === 'undefined') {{
            globalThis.__SSR_STREAM_SENTINEL = {{}};
        }}
        globalThis.__ssr_stream_result = globalThis.__SSR_STREAM_SENTINEL;
        globalThis.__ssr_stream_error = null;
        globalThis.__ssr_chunks = [];
        globalThis.__ssr_push_chunk = function(chunk) {{
            globalThis.__ssr_chunks.push(chunk);
        }};
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
        .execute_script("<ssr-deno:stream-chunked-start>", script.into())
        .map_err(|e| SSRDenoError::Render(format!("Chunked streaming render failed to start: {e}")))?;

    // Run the event loop — each tick, drain __ssr_chunks and forward to Ruby.
    let deadline = Instant::now() + Duration::from_millis(render_timeout_ms);

    let result = loop {
        if Instant::now() >= deadline {
            if oom_triggered.load(Ordering::SeqCst) {
                break Err(SSRDenoError::OutOfMemory(
                    "JS heap out of memory — the isolate reached its configured heap limit".into(),
                ));
            }
            break Err(SSRDenoError::Render("Chunked streaming render timed out".into()));
        }

        let remaining = deadline.saturating_duration_since(Instant::now());
        let tick = std::cmp::min(remaining, Duration::from_millis(50));
        let _ = worker.run_up_to_duration(tick).await;

        if oom_triggered.load(Ordering::SeqCst) {
            break Err(SSRDenoError::OutOfMemory(
                "JS heap out of memory — the isolate reached its configured heap limit".into(),
            ));
        }

        // Drain pending chunks from the JS array.
        drain_chunks(worker, &chunk_tx).await;

        match poll_stream_state(worker) {
            StreamState::Pending => continue,
            StreamState::Error(msg) => break Err(SSRDenoError::Render(msg)),
            StreamState::Done(_) => {
                // Final drain — the last event-loop tick may have produced
                // chunks that are sitting in __ssr_chunks after the promise
                // resolved.
                drain_chunks(worker, &chunk_tx).await;
                break Ok(());
            }
        }
    };

    // Clean up JS globals to avoid leaking state across renders.
    let _ = worker.execute_script(
        "<ssr-deno:stream-chunked-cleanup>",
        "globalThis.__ssr_chunks = undefined; globalThis.__ssr_push_chunk = undefined;"
            .to_string().into(),
    );

    // Drop chunk_tx (moved into this function) — closes the channel so the
    // Ruby consumer's `blocking_recv()` returns `None`, signaling EOS.
    drop(chunk_tx);

    result
}

/// Drains `globalThis.__ssr_chunks` and sends each element through `chunk_tx`.
/// Uses a single `execute_script` call that returns a JSON array of the
/// pending chunks, then clears the JS array.
async fn drain_chunks(worker: &mut MainWorker, chunk_tx: &mpsc::Sender<String>) {
    let Ok(global_val) = worker.execute_script(
        "<ssr-deno:stream-drain>",
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

    // Drop the scope before sending — v8 handles can't cross await points.
    drop(context_scope);
    drop(scope);

    // Parse the JSON array of chunk strings.
    if let Ok(chunks) = serde_json::from_str::<Vec<String>>(&json_str) {
        for chunk in chunks {
            // send().await: cannot use blocking_send inside async context.
            let _ = chunk_tx.send(chunk).await;
        }
    }
}
