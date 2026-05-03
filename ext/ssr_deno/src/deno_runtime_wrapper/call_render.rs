use deno_runtime::deno_core::v8;
use deno_runtime::worker::MainWorker;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use super::DenoError;

// ---------------------------------------------------------------------------
// call_render (sync + async)
// ---------------------------------------------------------------------------

struct AsyncHandle {
    global_promise: v8::Global<v8::Promise>,
    was_pending: bool,
}

pub fn call_render(
    worker: &mut MainWorker,
    bundle_id: &str,
    args_json: &str,
    render_timeout_ms: u64,
    oom_triggered: &AtomicBool,
) -> Result<String, DenoError> {
    let js_runtime = &mut worker.js_runtime;
    let context = js_runtime.main_context();
    let isolate = js_runtime.v8_isolate();

    // Raw Isolate pointer for Global::new inside scope chain.
    // TryCatch holds `&mut context_scope` blocking `&Isolate` borrow from
    // the scope.  SAFETY: isolate lives for entire function.
    let isolate_raw: *const v8::Isolate = &**isolate as *const v8::Isolate;

    // ══════════════════════════════════════════════════════════════════
    // Phase 1 — scope chain alive (block-scoped to release isolate borrow)
    let async_handle: Option<AsyncHandle> = {
        let mut scope_storage = std::pin::pin!(v8::HandleScope::new(isolate));
        let mut scope = scope_storage.as_mut().init();
        let context_local = v8::Local::new(&mut scope, &context);
        let mut context_scope = v8::ContextScope::new(&mut scope, context_local);

        let global = context_local.global(&mut context_scope);

        // ── Traversal helper: get a property, filter undefined/null ──────
        let mut get_prop = |obj: v8::Local<v8::Object>, key: &str| -> Result<v8::Local<v8::Value>, DenoError> {
            let k = v8::String::new(&mut context_scope, key).unwrap();
            obj.get(&mut context_scope, k.into())
                .filter(|v| !v.is_undefined() && !v.is_null())
                .ok_or_else(|| {
                    DenoError::BundleNotFound(
                        format!("Property '{key}' not found on SSR object (id: {bundle_id})"),
                    )
                })
        };

        let bundles_val = get_prop(global, "__ssr_bundles")?;
        let bundles_obj: v8::Local<v8::Object> = bundles_val.try_into().map_err(|_| {
            DenoError::BundleNotFound(format!("__ssr_bundles is not an object (id: {bundle_id})"))
        })?;

        let entry_val = get_prop(bundles_obj, bundle_id)?;
        let entry_obj: v8::Local<v8::Object> = entry_val.try_into().map_err(|_| {
            DenoError::BundleNotFound(format!("Bundle '{bundle_id}' entry is not an object"))
        })?;

        let render_val = get_prop(entry_obj, "render")?;
        let render_fn: v8::Local<v8::Function> = render_val.try_into().map_err(|_| {
            DenoError::BundleNotFound(format!("Bundle '{bundle_id}' render is not a function"))
        })?;

        let args_v8 = v8::String::new(&mut context_scope, args_json).unwrap();
        let undefined = v8::undefined(&mut context_scope);

        // TryCatch prevents V8 from marking the exception as unhandled.
        let mut tc = std::pin::pin!(v8::TryCatch::new(&mut context_scope));
        let try_catch = tc.as_mut().init();

        let call_result = render_fn.call(&try_catch, undefined.into(), &[args_v8.into()]);

        let result = match call_result {
            Some(v) => v,
            None => {
                if oom_triggered.load(Ordering::SeqCst) {
                    return Err(DenoError::OutOfMemory(
                        "JS heap out of memory — the isolate reached its configured heap limit".into(),
                    ));
                }
                let msg = try_catch
                    .message()
                    .map(|m| m.get(&try_catch).to_rust_string_lossy(&try_catch))
                    .unwrap_or_else(|| "`render` function threw an exception".to_string());
                return Err(DenoError::Render(msg));
            }
        };

        if let Ok(promise) = v8::Local::<v8::Promise>::try_from(result) {
            match promise.state() {
                v8::PromiseState::Fulfilled => {
                    let resolved = promise.result(&try_catch);
                    let json_str = v8::json::stringify(&try_catch, resolved)
                        .ok_or_else(|| DenoError::Render(
                            "Cannot serialize render result to JSON".to_string()
                        ))?;
                    return Ok(json_str.to_rust_string_lossy(&try_catch));
                }
                v8::PromiseState::Rejected => {
                    let global_promise = v8::Global::new(unsafe { &*isolate_raw }, promise);
                    Some(AsyncHandle { global_promise, was_pending: false })
                }
                v8::PromiseState::Pending => {
                    let global_promise = v8::Global::new(unsafe { &*isolate_raw }, promise);
                    Some(AsyncHandle { global_promise, was_pending: true })
                }
            }
        } else {
            let json_str = v8::json::stringify(&try_catch, result).ok_or_else(|| {
                DenoError::Render("Cannot serialize render result to JSON".to_string())
            })?;
            return Ok(json_str.to_rust_string_lossy(&try_catch));
        }
    }; // │ scope chain dropped — isolate borrow released

    // ══════════════════════════════════════════════════════════════════
    // Phase 2 — isolate free (scope chain dead)

    let AsyncHandle {
        global_promise,
        was_pending,
    } = async_handle.expect("async_handle is Some when we reach Phase 2");

    if was_pending {
        let deadline = Instant::now() + Duration::from_millis(render_timeout_ms);

        while Instant::now() < deadline {
            isolate.perform_microtask_checkpoint();

            let promise_ref = global_promise.open(isolate);

            match promise_ref.state() {
                v8::PromiseState::Pending => {
                    std::thread::sleep(Duration::from_micros(100));
                }
                _ => break,
            }
        }

        // Timeout check before re-entering scope chain.
        let promise_ref = global_promise.open(isolate);
        if promise_ref.state() == v8::PromiseState::Pending {
            if oom_triggered.load(Ordering::SeqCst) {
                return Err(DenoError::OutOfMemory(
                    "JS heap out of memory — the isolate reached its configured heap limit".into(),
                ));
            }
            return Err(DenoError::Render(
                format!("Async render promise did not settle within {render_timeout_ms}ms timeout"),
            ));
        }
    }

    {
        let mut scope_storage = std::pin::pin!(v8::HandleScope::new(isolate));
        let mut scope = scope_storage.as_mut().init();
        let context_local = v8::Local::new(&mut scope, &context);
        let mut context_scope = v8::ContextScope::new(&mut scope, context_local);

        // Open the Global through the scope chain to get &mut Isolate.
        let promise_ref = global_promise.open(AsMut::<v8::Isolate>::as_mut(&mut *context_scope));

        // Early exit above guarantees we never reach Pending here,
        // but match arms must be exhaustive.
        match promise_ref.state() {
            v8::PromiseState::Fulfilled => {
                let resolved = promise_ref.result(&context_scope);
                let json_str =
                    v8::json::stringify(&mut context_scope, resolved).ok_or_else(|| {
                        if oom_triggered.load(Ordering::SeqCst) {
                            DenoError::OutOfMemory(
                                "JS heap out of memory — the isolate reached its configured heap limit".into(),
                            )
                        } else {
                            DenoError::Render("Cannot serialize render result to JSON".to_string())
                        }
                    })?;
                Ok(json_str.to_rust_string_lossy(&mut context_scope))
            }
            v8::PromiseState::Rejected => {
                if oom_triggered.load(Ordering::SeqCst) {
                    return Err(DenoError::OutOfMemory(
                        "JS heap out of memory — the isolate reached its configured heap limit".into(),
                    ));
                }
                let rejection = promise_ref.result(&context_scope);
                let msg = if rejection.is_string() {
                    rejection.to_rust_string_lossy(&mut context_scope)
                } else if rejection.is_object() {
                    v8::json::stringify(&mut context_scope, rejection)
                        .map(|s| s.to_rust_string_lossy(&mut context_scope))
                        .unwrap_or_else(|| "Promise rejected (non-serializable value)".to_string())
                } else {
                    "Promise rejected".to_string()
                };
                Err(DenoError::Render(msg))
            }
            v8::PromiseState::Pending => unreachable!("timeout checked before Phase 2"),
        }
    }
}

// ---------------------------------------------------------------------------
// V8 heap statistics
// ---------------------------------------------------------------------------

pub fn collect_heap_stats(worker: &mut MainWorker) -> Result<String, DenoError> {
    let js_runtime = &mut worker.js_runtime;
    let isolate = js_runtime.v8_isolate();
    let stats = isolate.get_heap_statistics();

    let stats_json = serde_json::json!({
        "total_heap_size": stats.total_heap_size(),
        "total_heap_size_executable": stats.total_heap_size_executable(),
        "total_physical_size": stats.total_physical_size(),
        "total_available_size": stats.total_available_size(),
        "used_heap_size": stats.used_heap_size(),
        "heap_size_limit": stats.heap_size_limit(),
        "malloced_memory": stats.malloced_memory(),
        "external_memory": stats.external_memory(),
        "peak_malloced_memory": stats.peak_malloced_memory(),
        "number_of_native_contexts": stats.number_of_native_contexts(),
        "number_of_detached_contexts": stats.number_of_detached_contexts(),
        "total_global_handles_size": stats.total_global_handles_size(),
        "used_global_handles_size": stats.used_global_handles_size(),
    });

    Ok(stats_json.to_string())
}
