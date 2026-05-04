use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use deno_core::op2;
use deno_core::OpState;
use deno_error::JsErrorBox;
use deno_runtime::deno_core::v8;
use deno_runtime::worker::MainWorker;
use tokio::sync::mpsc;

use super::SSRDenoError;
use super::watchdog::Watchdog;

// ---------------------------------------------------------------------------
// Op: receive a chunk of HTML from JS during chunked render
// ---------------------------------------------------------------------------

/// Pushes an HTML chunk from JS to the Rust channel. Async for backpressure:
/// when the channel buffer (64 slots) is full, the JS call awaits until the
/// Ruby consumer drains a slot. This prevents OOM from fast-producing React +
/// slow-consuming client.
///
/// Registered globally in the extension but only active when `chunk_tx` is
/// placed in OpState (i.e., during `render_chunked` execution).
///
/// JS usage: `await Deno.core.ops.op_ssr_push_chunk(chunkString)`
#[op2]
pub async fn op_ssr_push_chunk(
    #[string] chunk: String,
    state: Rc<RefCell<OpState>>,
) -> Result<(), JsErrorBox> {
    let tx = {
        let op_state = state.borrow();
        op_state.borrow::<mpsc::Sender<String>>().clone()
    };

    tx.send(chunk).await.map_err(|_| {
        JsErrorBox::generic("op_ssr_push_chunk: channel closed")
    })
}

// ---------------------------------------------------------------------------
// Render — event-loop based, returns final result as JSON string
// ---------------------------------------------------------------------------

/// Cleans up render-state globals to prevent leakage across renders.
pub(super) fn cleanup_render_globals(worker: &mut MainWorker) {
    let _ = worker.execute_script(
        "<ssr-deno:render-cleanup>",
        "globalThis.__ssr_deno_result = undefined; \
         globalThis.__ssr_deno_error = undefined;"
            .to_string()
            .into(),
    );
}

/// Runs a render with the full Deno event loop. The bundle's `render` function
/// is called with `args_json`. If it returns a Promise, the event loop runs
/// until the Promise settles (or the timeout expires). Macrotasks like
/// `setTimeout`, `setInterval`, and `MessageChannel` fire normally.
///
/// A watchdog thread monitors the render timeout. If the JS code blocks
/// synchronously (e.g., an infinite `while` loop), the watchdog calls
/// `terminate_execution()` from a separate thread — this is the only way to
/// interrupt V8 execution that is stuck inside a synchronous computation.
///
/// Returns the final result as a JSON-stringified value.
pub async fn render(
    worker: &mut MainWorker,
    bundle_id: &str,
    args_json: &str,
    render_timeout_ms: u64,
    oom_triggered: &AtomicBool,
) -> Result<String, SSRDenoError> {
    // Use serde_json for bundle_id injection — produces a guaranteed-valid JS
    // string literal regardless of special characters in the filename.
    let bundle_id_js = serde_json::to_string(bundle_id)
        .unwrap_or_else(|_| format!("\"{}\"", bundle_id));

    // Kick off the render. The bundle's render function is stored at
    // globalThis.__ssr_bundles[bundle_id].render. It receives the args as a
    // JSON string (same contract as the direct V8 API call path).
    //
    // Error handling: rejected promises store the error message in a
    // separate `__ssr_deno_error` global (not in `__ssr_deno_result`).
    // The poll loop checks `__ssr_deno_error` first and returns
    // `SSRDenoError::Render` when set, ensuring proper exception propagation
    // back to Ruby.
    let args_json_js = serde_json::to_string(args_json)
        .unwrap_or_else(|_| format!("\"{}\"", args_json));

    let script = format!(
        r#"
        if (typeof globalThis.__SSR_DENO_SENTINEL === 'undefined') {{
            globalThis.__SSR_DENO_SENTINEL = {{}};
        }}
        globalThis.__ssr_deno_result = globalThis.__SSR_DENO_SENTINEL;
        globalThis.__ssr_deno_error = null;
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

    // Arm the watchdog before execute_script — this covers sync-blocking
    // renders that never yield back to the event loop.
    let v8_handle = worker.js_runtime.v8_isolate().thread_safe_handle();
    let timeout_triggered = Arc::new(AtomicBool::new(false));
    let watchdog = Watchdog::spawn(v8_handle, render_timeout_ms, timeout_triggered.clone());

    let exec_result = worker
        .execute_script("<ssr-deno:render-start>", script.into());

    // If execute_script itself was terminated (sync render blocked too long),
    // handle the termination before entering the event loop.
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
            return Err(SSRDenoError::Render("Render timed out".into()));
        }

        let msg = e.to_string();
        return if msg.contains("Bundle not found:") {
            Err(SSRDenoError::BundleNotFound(msg))
        } else {
            Err(SSRDenoError::Render(format!("Render failed to start: {msg}")))
        };
    }

    // Run the event loop until the render completes or the watchdog fires.
    // The watchdog is the sole timeout authority — no separate deadline check
    // here. This eliminates races between two timeout mechanisms.
    let result = loop {
        // Run the event loop briefly to let macrotasks/promises progress.
        let _ = worker.run_up_to_duration(Duration::from_millis(50)).await;

        if oom_triggered.load(Ordering::SeqCst) {
            worker.js_runtime.v8_isolate().cancel_terminate_execution();
            break Err(SSRDenoError::OutOfMemory(
                "JS heap out of memory - the isolate reached its configured heap limit".into(),
            ));
        }
        if timeout_triggered.load(Ordering::SeqCst) {
            worker.js_runtime.v8_isolate().cancel_terminate_execution();
            break Err(SSRDenoError::Render("Render timed out".into()));
        }

        // Single script call per tick: checks error first, then result.
        // Returns null when still pending, "E:<msg>" on error, or
        // "R:<json>" when the render completed successfully.
        match poll_render_state(worker) {
            RenderState::Pending => continue,
            RenderState::Error(msg) => break Err(SSRDenoError::Render(msg)),
            RenderState::Done(result) => break Ok(result),
        }
    };

    watchdog.cancel();

    // If the watchdog or OOM callback fired between the loop's deadline check
    // and watchdog.cancel(), the isolate has pending termination. Clear it so
    // the isolate is reusable for future operations.
    if timeout_triggered.load(Ordering::SeqCst) || oom_triggered.load(Ordering::SeqCst) {
        worker.js_runtime.v8_isolate().cancel_terminate_execution();
    }

    cleanup_render_globals(worker);

    result
}

// ---------------------------------------------------------------------------
// Render state polling — shared by render and render_chunked
// ---------------------------------------------------------------------------

pub(super) enum RenderState {
    Pending,
    Error(String),
    Done(String),
}

/// Checks both `__ssr_deno_error` and `__ssr_deno_result` in a single
/// `execute_script` call. Returns the render state as a tagged string:
/// - `null` / undefined - still pending
/// - starts with `E:` - promise rejected, rest is the error message
/// - starts with `R:` - render complete, rest is the JSON result
pub(super) fn poll_render_state(worker: &mut MainWorker) -> RenderState {
    let Ok(global_val) = worker.execute_script(
        "<ssr-deno:render-poll>",
        "globalThis.__ssr_deno_error \
         ? ('E:' + globalThis.__ssr_deno_error) \
         : (globalThis.__ssr_deno_result === globalThis.__SSR_DENO_SENTINEL \
            ? null \
            : ('R:' + JSON.stringify(globalThis.__ssr_deno_result)))"
            .to_string()
            .into(),
    ) else {
        // Script execution failed — treat as pending (next tick will retry
        // or the timeout will fire).
        return RenderState::Pending;
    };

    let context = worker.js_runtime.main_context();
    let isolate = worker.js_runtime.v8_isolate();
    let mut scope_storage = std::pin::pin!(v8::HandleScope::new(isolate));
    let mut scope = scope_storage.as_mut().init();
    let context_local = v8::Local::new(&mut scope, &context);
    let mut context_scope = v8::ContextScope::new(&mut scope, context_local);

    let local_val = v8::Local::new(&mut context_scope, &global_val);

    if local_val.is_null_or_undefined() {
        return RenderState::Pending;
    }

    let s = local_val.to_rust_string_lossy(&mut context_scope);

    if let Some(err_msg) = s.strip_prefix("E:") {
        RenderState::Error(err_msg.to_string())
    } else if let Some(result) = s.strip_prefix("R:") {
        RenderState::Done(result.to_string())
    } else {
        // Unrecognised format — the poll JS returned something unexpected.
        // Surface this as an error to avoid infinite polling until timeout.
        RenderState::Error(format!(
            "Render state poll returned unrecognised value (prefix: {})",
            s.chars().take(20).collect::<String>()
        ))
    }
}
