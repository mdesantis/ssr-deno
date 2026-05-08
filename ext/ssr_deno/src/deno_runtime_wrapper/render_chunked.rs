use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use deno_runtime::deno_core::v8;
use deno_runtime::worker::MainWorker;
use tokio::sync::mpsc;

use super::SSRDenoError;
use super::render::{
    begin_render, end_render, poll_render_state, to_js_string, RenderState,
};

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
    let bundle_id_js = to_js_string(bundle_id);
    let args_json_js = to_js_string(args_json);

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

    let (watchdog, timeout_triggered) = begin_render(
        worker, script, "<ssr-deno:render-chunked-start>", render_timeout_ms, oom_triggered, "chunked-render",
    ).inspect_err(|_| {
        let _ = worker.execute_script(
            "<ssr-deno:render-chunked-cleanup>",
            "globalThis.__ssr_deno_result = undefined; \
             globalThis.__ssr_deno_error = undefined; \
             globalThis.__ssr_chunks = undefined; \
             globalThis.__ssr_push_chunk = undefined;"
                .to_string()
                .into(),
        );
    })?;

    // Run the event loop -- each tick, drain __ssr_chunks and forward to Ruby.
    // The watchdog is the sole timeout authority — no separate deadline check.
    let result = loop {
        let _ = worker.run_up_to_duration(Duration::from_millis(50)).await;

        // OOM checked before timeout — when both fire concurrently
        // OOM is the root cause (V8 near-heap-limit callback sets flag
        // and terminates execution). Returning OOM is more specific.
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

    end_render(worker, watchdog, &timeout_triggered, oom_triggered);

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
    let context_local = v8::Local::new(&scope, &context);
    let context_scope = v8::ContextScope::new(&mut scope, context_local);

    let local_val = v8::Local::new(&context_scope, &global_val);

    if local_val.is_null_or_undefined() {
        return;
    }

    let json_str = local_val.to_rust_string_lossy(&context_scope);

    drop(context_scope);

    // Parse the JSON array of chunk strings.
    // On error the if-let silently falls through — V8's JSON.stringify cannot
    // produce invalid JSON for a well-formed array, so this is unreachable in
    // practice. A corrupt V8 heap is the only theoretical path here, and logging
    // from within deno_core Ops is impractical. We accept the silent drop rather
    // than adding error-handling plumbing for an unreachable edge case.
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
