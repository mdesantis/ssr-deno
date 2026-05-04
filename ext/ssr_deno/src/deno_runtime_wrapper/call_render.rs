use deno_runtime::deno_core::v8;
use deno_runtime::worker::MainWorker;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use super::SSRDenoError;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extracts a human-readable error message from a rejected Promise's result value.
/// Handles string rejections, object rejections (JSON-serialized), and other types.
///
/// This is a macro rather than a function because V8's scope types are
/// parameterized differently (TryCatch vs ContextScope) and the API functions
/// (`to_rust_string_lossy`, `v8::json::stringify`) accept `&PinScope` via blanket
/// Deref impls that don't unify into a single function signature easily.
macro_rules! extract_rejection_msg {
    ($scope:expr, $rejection:expr) => {{
        let rejection = $rejection;
        if rejection.is_string() {
            rejection.to_rust_string_lossy(&$scope)
        } else if rejection.is_object() {
            v8::json::stringify(&$scope, rejection)
                .map(|s| s.to_rust_string_lossy(&$scope))
                .unwrap_or_else(|| "Promise rejected (non-serializable value)".to_string())
        } else {
            "Promise rejected".to_string()
        }
    }};
}

// ---------------------------------------------------------------------------
// call_render (sync + async) — orchestration
// ---------------------------------------------------------------------------

enum Phase1Outcome {
    Sync(String),
    Pending { promise: v8::Global<v8::Promise> },
}

/// Look up the render function, call it, and dispatch the result.
/// - Non-Promise return → stringifies directly → Ok(Sync(s))
/// - Resolved Promise → reads result in Phase 1 scope → Ok(Sync(s))
/// - Rejected Promise → extracts error via helper → Err(SSRDenoError)
/// - Pending Promise → saves Global, returns Ok(Pending { promise })
fn phase1_lookup_and_call(
    isolate: &mut v8::OwnedIsolate,
    context: &v8::Global<v8::Context>,
    bundle_id: &str,
    args_json: &str,
    oom_triggered: &AtomicBool,
) -> Result<Phase1Outcome, SSRDenoError> {
    let result = {
        let mut scope_storage = std::pin::pin!(v8::HandleScope::new(isolate));
        let mut scope = scope_storage.as_mut().init();
        let context_local = v8::Local::new(&mut scope, context);
        let mut context_scope = v8::ContextScope::new(&mut scope, context_local);

        let global = context_local.global(&mut context_scope);

        let mut get_prop = |obj: v8::Local<v8::Object>, key: &str| -> Result<v8::Local<v8::Value>, SSRDenoError> {
            let k = v8::String::new(&mut context_scope, key).unwrap();
            obj.get(&mut context_scope, k.into())
                .filter(|v| !v.is_undefined() && !v.is_null())
                .ok_or_else(|| {
                    SSRDenoError::BundleNotFound(
                        format!("Property '{key}' not found on SSR object (id: {bundle_id})"),
                    )
                })
        };

        let bundles_val = get_prop(global, "__ssr_bundles")?;
        let bundles_obj: v8::Local<v8::Object> = bundles_val.try_into().map_err(|_| {
            SSRDenoError::BundleNotFound(format!("__ssr_bundles is not an object (id: {bundle_id})"))
        })?;

        let entry_val = get_prop(bundles_obj, bundle_id)?;
        let entry_obj: v8::Local<v8::Object> = entry_val.try_into().map_err(|_| {
            SSRDenoError::BundleNotFound(format!("Bundle '{bundle_id}' entry is not an object"))
        })?;

        let render_val = get_prop(entry_obj, "render")?;
        let render_fn: v8::Local<v8::Function> = render_val.try_into().map_err(|_| {
            SSRDenoError::BundleNotFound(format!("Bundle '{bundle_id}' render is not a function"))
        })?;

        let args_v8 = v8::String::new(&mut context_scope, args_json).unwrap();
        let undefined = v8::undefined(&mut context_scope);

        let mut tc = std::pin::pin!(v8::TryCatch::new(&mut context_scope));
        let try_catch = tc.as_mut().init();

        let call_result = render_fn.call(&try_catch, undefined.into(), &[args_v8.into()]);

        let result = match call_result {
            Some(v) => v,
            None => {
                if oom_triggered.load(Ordering::SeqCst) {
                    return Err(SSRDenoError::OutOfMemory(
                        "JS heap out of memory — the isolate reached its configured heap limit".into(),
                    ));
                }
                let msg = try_catch
                    .message()
                    .map(|m| m.get(&try_catch).to_rust_string_lossy(&try_catch))
                    .unwrap_or_else(|| "`render` function threw an exception".to_string());
                return Err(SSRDenoError::Render(msg));
            }
        };

        if let Ok(promise) = v8::Local::<v8::Promise>::try_from(result) {
            match promise.state() {
                v8::PromiseState::Fulfilled => {
                    let resolved = promise.result(&try_catch);
                    let json_str = v8::json::stringify(&try_catch, resolved)
                        .ok_or_else(|| SSRDenoError::Render(
                            "Cannot serialize render result to JSON".to_string()
                        ))?;
                    return Ok(Phase1Outcome::Sync(json_str.to_rust_string_lossy(&try_catch)));
                }
                v8::PromiseState::Rejected => {
                    let rejection = promise.result(&try_catch);
                    if oom_triggered.load(Ordering::SeqCst) {
                        return Err(SSRDenoError::OutOfMemory(
                            "JS heap out of memory — the isolate reached its configured heap limit".into(),
                        ));
                    }
                    let msg = extract_rejection_msg!(try_catch, rejection);
                    return Err(SSRDenoError::Render(msg));
                }
                v8::PromiseState::Pending => {
                    let global_promise = v8::Global::new(try_catch.as_ref(), promise);
                    Ok(Phase1Outcome::Pending { promise: global_promise })
                }
            }
        } else {
            let json_str = v8::json::stringify(&try_catch, result).ok_or_else(|| {
                SSRDenoError::Render("Cannot serialize render result to JSON".to_string())
            })?;
            return Ok(Phase1Outcome::Sync(json_str.to_rust_string_lossy(&try_catch)));
        }
    };

    result
}

/// Poll the microtask queue until the promise settles or the deadline expires,
/// then re-enter the scope chain and extract the result.
fn phase2_poll_and_resolve(
    isolate: &mut v8::OwnedIsolate,
    context: &v8::Global<v8::Context>,
    promise: v8::Global<v8::Promise>,
    render_timeout_ms: u64,
    oom_triggered: &AtomicBool,
) -> Result<String, SSRDenoError> {
    let deadline = Instant::now() + Duration::from_millis(render_timeout_ms);

    while Instant::now() < deadline {
        isolate.perform_microtask_checkpoint();

        let promise_ref = promise.open(isolate);

        match promise_ref.state() {
            v8::PromiseState::Pending => {
                std::thread::sleep(Duration::from_micros(100));
            }
            _ => break,
        }
    }

    // Timeout check before re-entering scope chain.
    let promise_ref = promise.open(isolate);
    if promise_ref.state() == v8::PromiseState::Pending {
        if oom_triggered.load(Ordering::SeqCst) {
            return Err(SSRDenoError::OutOfMemory(
                "JS heap out of memory — the isolate reached its configured heap limit".into(),
            ));
        }
        return Err(SSRDenoError::Render(
            format!("Async render promise did not settle within {render_timeout_ms}ms timeout"),
        ));
    }

    // Re-enter scope chain to read the promise result.
    let mut scope_storage = std::pin::pin!(v8::HandleScope::new(isolate));
    let mut scope = scope_storage.as_mut().init();
    let context_local = v8::Local::new(&mut scope, context);
    let mut context_scope = v8::ContextScope::new(&mut scope, context_local);

    let promise_ref = promise.open(AsMut::<v8::Isolate>::as_mut(&mut *context_scope));

    // Timeout above guarantees we never reach Pending here.
    match promise_ref.state() {
        v8::PromiseState::Fulfilled => {
            let resolved = promise_ref.result(&context_scope);
            let json_str =
                v8::json::stringify(&mut context_scope, resolved).ok_or_else(|| {
                    if oom_triggered.load(Ordering::SeqCst) {
                        SSRDenoError::OutOfMemory(
                            "JS heap out of memory — the isolate reached its configured heap limit".into(),
                        )
                    } else {
                        SSRDenoError::Render("Cannot serialize render result to JSON".to_string())
                    }
                })?;
            Ok(json_str.to_rust_string_lossy(&mut context_scope))
        }
        v8::PromiseState::Rejected => {
            let rejection = promise_ref.result(&context_scope);
            if oom_triggered.load(Ordering::SeqCst) {
                return Err(SSRDenoError::OutOfMemory(
                    "JS heap out of memory — the isolate reached its configured heap limit".into(),
                ));
            }
            let msg = extract_rejection_msg!(context_scope, rejection);
            Err(SSRDenoError::Render(msg))
        }
        v8::PromiseState::Pending => unreachable!("timeout checked before scope chain re-entry"),
    }
}

pub fn call_render(
    worker: &mut MainWorker,
    bundle_id: &str,
    args_json: &str,
    render_timeout_ms: u64,
    oom_triggered: &AtomicBool,
) -> Result<String, SSRDenoError> {
    let js_runtime = &mut worker.js_runtime;
    let context = js_runtime.main_context();
    let isolate = js_runtime.v8_isolate();

    match phase1_lookup_and_call(isolate, &context, bundle_id, args_json, oom_triggered)? {
        Phase1Outcome::Sync(s) => Ok(s),
        Phase1Outcome::Pending { promise } => {
            phase2_poll_and_resolve(isolate, &context, promise, render_timeout_ms, oom_triggered)
        }
    }
}

// ---------------------------------------------------------------------------
// V8 heap statistics
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct HeapStats {
    total_heap_size: usize,
    total_heap_size_executable: usize,
    total_physical_size: usize,
    total_available_size: usize,
    used_heap_size: usize,
    heap_size_limit: usize,
    malloced_memory: usize,
    external_memory: usize,
    peak_malloced_memory: usize,
    number_of_native_contexts: usize,
    number_of_detached_contexts: usize,
    total_global_handles_size: usize,
    used_global_handles_size: usize,
}

pub fn collect_heap_stats(worker: &mut MainWorker) -> Result<String, SSRDenoError> {
    let js_runtime = &mut worker.js_runtime;
    let isolate = js_runtime.v8_isolate();
    let stats = isolate.get_heap_statistics();

    let heap = HeapStats {
        total_heap_size: stats.total_heap_size(),
        total_heap_size_executable: stats.total_heap_size_executable(),
        total_physical_size: stats.total_physical_size(),
        total_available_size: stats.total_available_size(),
        used_heap_size: stats.used_heap_size(),
        heap_size_limit: stats.heap_size_limit(),
        malloced_memory: stats.malloced_memory(),
        external_memory: stats.external_memory(),
        peak_malloced_memory: stats.peak_malloced_memory(),
        number_of_native_contexts: stats.number_of_native_contexts(),
        number_of_detached_contexts: stats.number_of_detached_contexts(),
        total_global_handles_size: stats.total_global_handles_size(),
        used_global_handles_size: stats.used_global_handles_size(),
    };

    serde_json::to_string(&heap)
        .map_err(|e| SSRDenoError::HeapStatsSerialization(format!("Failed to serialize heap stats: {e}")))
}
