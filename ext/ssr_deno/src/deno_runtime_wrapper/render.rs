use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use deno_runtime::deno_core::v8;
use deno_runtime::worker::MainWorker;

use super::SSRDenoError;
use super::watchdog::Watchdog;

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

/// Produces a JS-safe string literal from a bundle_id or args_json.
/// Uses serde_json for guaranteed escaping, falls back to double-quoting.
pub(super) fn to_js_string(s: &str) -> String {
    serde_json::to_string(s).expect("serde_json::to_string cannot fail for &str")
}

/// Arms the watchdog, executes `startup_script`, and dispatches execution
/// errors (OOM, timeout, BundleNotFound, generic Render). Returns the
/// watchdog and timeout flag on success so the caller owns the lifecycle.
pub(super) fn begin_render(
    worker: &mut MainWorker,
    startup_script: String,
    script_name: &'static str,
    render_timeout_ms: u64,
    oom_triggered: &AtomicBool,
    error_label: &str,
) -> Result<(Watchdog, Arc<AtomicBool>), SSRDenoError> {
    let v8_handle = worker.js_runtime.v8_isolate().thread_safe_handle();
    let timeout_triggered = Arc::new(AtomicBool::new(false));
    let watchdog = Watchdog::spawn(v8_handle, render_timeout_ms, timeout_triggered.clone())
        .map_err(SSRDenoError::Render)?;

    let exec_result = worker.execute_script(script_name, startup_script.into());

    if let Err(e) = exec_result {
        watchdog.cancel();
        cleanup_render_globals(worker);

        // Check OOM before timeout — when both fire concurrently,
        // OOM is the root cause (V8 limit triggers terminate_execution).
        if oom_triggered.load(Ordering::SeqCst) {
            worker.js_runtime.v8_isolate().cancel_terminate_execution();
            return Err(SSRDenoError::OutOfMemory(format!(
                "{error_label} - JS heap out of memory"
            )));
        }
        if timeout_triggered.load(Ordering::SeqCst) {
            worker.js_runtime.v8_isolate().cancel_terminate_execution();
            return Err(SSRDenoError::Render(format!("{error_label} timed out")));
        }

        let msg = e.to_string();
        return if msg.contains("Bundle not found:") {
            Err(SSRDenoError::BundleNotFound(msg))
        } else {
            Err(SSRDenoError::Render(format!(
                "{error_label} failed to start: {msg}"
            )))
        };
    }

    Ok((watchdog, timeout_triggered))
}

/// Cancels the watchdog and clears any pending terminate_execution.
pub(super) fn end_render(
    worker: &mut MainWorker,
    watchdog: Watchdog,
    timeout_triggered: &AtomicBool,
    oom_triggered: &AtomicBool,
) {
    watchdog.cancel();
    if timeout_triggered.load(Ordering::SeqCst) || oom_triggered.load(Ordering::SeqCst) {
        worker.js_runtime.v8_isolate().cancel_terminate_execution();
    }
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
    let bundle_id_js = to_js_string(bundle_id);
    let args_json_js = to_js_string(args_json);

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

    let (watchdog, timeout_triggered) = begin_render(
        worker, script, "<ssr-deno:render-start>", render_timeout_ms, oom_triggered, "render",
    )?;

    // Run the event loop until the render completes or the watchdog fires.
    // The watchdog is the sole timeout authority — no separate deadline check
    // here. This eliminates races between two timeout mechanisms.
    let result = loop {
        // Run the event loop briefly to let macrotasks/promises progress.
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
            break Err(SSRDenoError::Render("Render timed out".into()));
        }

        match poll_render_state(worker) {
            RenderState::Pending => continue,
            RenderState::Error(msg) => break Err(SSRDenoError::Render(msg)),
            RenderState::Done(result) => break Ok(result),
        }
    };

    end_render(worker, watchdog, &timeout_triggered, oom_triggered);
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
    let context_local = v8::Local::new(&scope, &context);
    let context_scope = v8::ContextScope::new(&mut scope, context_local);

    let local_val = v8::Local::new(&context_scope, &global_val);

    if local_val.is_null_or_undefined() {
        return RenderState::Pending;
    }

    let s = local_val.to_rust_string_lossy(&context_scope);

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
