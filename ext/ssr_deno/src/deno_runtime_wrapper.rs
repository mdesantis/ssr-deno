use std::sync::atomic::AtomicUsize;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use deno_runtime::deno_core::url::Url;
use deno_runtime::deno_core::v8;
use deno_runtime::deno_permissions::Permissions;
use deno_runtime::deno_permissions::PermissionsContainer;
use deno_runtime::worker::MainWorker;
use deno_runtime::worker::WorkerOptions;
use deno_runtime::worker::WorkerServiceOptions;
use deno_runtime::BootstrapOptions;
use deno_runtime::FeatureChecker;

use crate::nop_types::NopInNpmPackageChecker;
use crate::nop_types::NopNpmPackageFolderResolver;
use crate::nop_types::NopPermissionDescriptorParser;
use crate::sys::Sys;

pub use ssr_deno_core::DenoError;
pub use ssr_deno_core::{next_index, validate_pool_size};
// MAX_ISOLATES is available through ssr_deno_core::MAX_ISOLATES if needed.

// ---------------------------------------------------------------------------
// Wire protocol between the Ruby thread and each Deno worker thread
// ---------------------------------------------------------------------------

enum WorkerMsg {
    LoadBundle {
        bundle_id: String,
        bundle_code: String,
        script_name: &'static str,
        reply: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
    Render {
        bundle_id: String,
        args_json: String,
        reply: std::sync::mpsc::SyncSender<Result<String, DenoError>>,
    },
}

// ---------------------------------------------------------------------------
// IsolateHandle — per-isolate channel sender
// ---------------------------------------------------------------------------

/// Owns the channel to a dedicated background thread that runs a single
/// Deno `MainWorker` (V8 isolate + Web API extensions).
///
/// Because `MainWorker` never leaves its thread, no `unsafe` impl or
/// `UnsafeCell` is required — `tokio::sync::mpsc::Sender` is `Send + Sync`
/// on its own.
pub struct IsolateHandle {
    tx: tokio::sync::mpsc::Sender<WorkerMsg>,
    render_timeout_ms: u64,
}

impl IsolateHandle {
    /// Spawns a Deno worker thread with the given index and heap limit.
    /// Blocks until the worker is ready to accept messages.
    pub fn spawn(index: usize, max_heap_size_mb: usize, render_timeout_ms: u64) -> Result<Self, DenoError> {
        let (tx, rx) = tokio::sync::mpsc::channel::<WorkerMsg>(1);
        let (init_tx, init_rx) = mpsc::sync_channel::<Result<(), String>>(1);

        std::thread::Builder::new()
            .name(format!("deno-worker-{index}"))
            .spawn(move || worker_thread_main(rx, init_tx, max_heap_size_mb))
            .map_err(|e| {
                DenoError::WorkerInit(format!("Failed to spawn isolate thread {index}: {e}"))
            })?;

        init_rx
            .recv()
            .map_err(|_| {
                DenoError::WorkerInit("Isolate thread exited unexpectedly during init".into())
            })?
            .map_err(DenoError::WorkerInit)?;

        Ok(Self { tx, render_timeout_ms })
    }

    /// Sends a render request to this isolate's worker thread and blocks
    /// until the result arrives. Returns the result as a JSON string so any
    /// JS type survives the boundary.
    pub fn block_on_render(&self, bundle_id: &str, args_json: &str) -> Result<String, DenoError> {
        let (reply_tx, reply_rx) =
            std::sync::mpsc::sync_channel::<Result<String, DenoError>>(1);
        let timeout = Duration::from_millis(self.render_timeout_ms);

        self.tx
            .blocking_send(WorkerMsg::Render {
                bundle_id: bundle_id.to_string(),
                args_json: args_json.to_string(),
                reply: reply_tx,
            })
            .map_err(|_| DenoError::WorkerDied("Deno worker thread has exited".into()))?;

        match reply_rx.recv_timeout(timeout) {
            Ok(result) => result,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                Err(DenoError::Render(
                    format!("Render timed out after {}ms", timeout.as_millis()),
                ))
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                Err(DenoError::WorkerDied(
                    "Deno worker thread exited before sending a reply".into(),
                ))
            }
        }
    }

    /// Low-level send of a WorkerMsg. Used by IsolatePool for bundle broadcast.
    fn blocking_send(&self, msg: WorkerMsg) -> Result<(), DenoError> {
        self.tx
            .blocking_send(msg)
            .map_err(|_| DenoError::WorkerDied("Isolate worker has exited".into()))
    }
}

// ---------------------------------------------------------------------------
// IsolatePool — dispatcher of render requests across N isolates
// ---------------------------------------------------------------------------

/// A load-balancing dispatcher that owns multiple `IsolateHandle`s and
/// distributes render requests across them in round-robin fashion.
pub struct IsolatePool {
    handles: Vec<IsolateHandle>,
    counter: AtomicUsize, // Round-robin counter
}

impl IsolatePool {
    /// Creates a pool of `size` isolates, each with `max_heap_size_mb`
    /// as its V8 heap limit and `render_timeout_ms` as the render timeout.
    /// Returns an error if `size` is 0 or if any
    /// isolate thread fails to spawn.
    pub fn new(size: usize, max_heap_size_mb: usize, render_timeout_ms: u64) -> Result<Self, DenoError> {
        validate_pool_size(size)?;

        let mut handles = Vec::with_capacity(size);
        for i in 0..size {
            let handle = IsolateHandle::spawn(i, max_heap_size_mb, render_timeout_ms)?;
            handles.push(handle);
        }

        Ok(Self {
            handles,
            counter: AtomicUsize::new(0),
        })
    }

    /// Returns the number of live isolates in the pool.
    /// Currently unused externally — will be needed by heap_stats_all
    /// for per-isolate metrics reporting (see v8-heap-metrics.md).
    #[allow(dead_code)]
    pub fn size(&self) -> usize {
        self.handles.len()
    }

    /// Picks the next isolate in round-robin order.
    fn next_handle(&self) -> &IsolateHandle {
        let idx = next_index(&self.counter, self.handles.len());
        &self.handles[idx]
    }

    /// Dispatches a render request to the next available isolate.
    /// Blocks until the result arrives.
    pub fn dispatch_render(&self, bundle_id: &str, args_json: &str) -> Result<String, DenoError> {
        self.next_handle().block_on_render(bundle_id, args_json)
    }

    /// Loads a bundle into **every** isolate by broadcasting the bundle code.
    /// Path resolution (canonicalize, symlink check) is done once — all
    /// isolates receive the same code and script name.
    pub fn load_bundle(&self, bundle_id: &str, bundle_path: &str) -> Result<(), DenoError> {
        let bundle_name = std::path::Path::new(bundle_path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("(unknown)");
        let canonical = std::fs::canonicalize(bundle_path).map_err(|e| {
            DenoError::BundleLoad(format!("Cannot resolve bundle path '{bundle_name}': {e}"))
        })?;

        // Reject symlink escapes: the resolved path must stay within the
        // directory that was originally specified.
        let original_parent = std::path::Path::new(bundle_path)
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(std::path::Path::new("."));
        let canonical_parent = std::fs::canonicalize(original_parent)
            .map_err(|e| DenoError::BundleLoad(format!("Cannot resolve bundle directory: {e}")))?;
        if !canonical.starts_with(&canonical_parent) {
            return Err(DenoError::BundleLoad(format!(
                "Bundle file '{bundle_name}' escapes its directory via symlink"
            )));
        }

        let bundle_code = std::fs::read_to_string(bundle_path).map_err(|e| {
            DenoError::BundleLoad(format!("Cannot read bundle file '{bundle_name}': {e}"))
        })?;

        // `MainWorker::execute_script` requires `&'static str` for the script
        // name. One bounded leak per bundle load (shared by all isolates).
        let script_name: &'static str = canonical
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| Box::leak(s.to_owned().into_boxed_str()) as &'static str)
            .unwrap_or("main.js");

        // Broadcast to all isolates (sequential — keeps things simple for v1).
        for handle in &self.handles {
            let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();

            handle.blocking_send(WorkerMsg::LoadBundle {
                bundle_id: bundle_id.to_string(),
                bundle_code: bundle_code.clone(),
                script_name,
                reply: reply_tx,
            })?;

            reply_rx
                .blocking_recv()
                .map_err(|_| DenoError::WorkerDied("Isolate worker exited before reply".into()))?
                .map_err(DenoError::BundleLoad)?;
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Worker thread (per-isolate)
// ---------------------------------------------------------------------------

fn worker_thread_main(
    mut rx: tokio::sync::mpsc::Receiver<WorkerMsg>,
    init_tx: mpsc::SyncSender<Result<(), String>>,
    max_heap_size_mb: usize,
) {
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            let _ = init_tx.send(Err(format!("Failed to build Tokio runtime: {e}")));
            return;
        }
    };

    // LocalSet is required by deno_unsync::spawn_local, which Deno's Web API
    // extensions (e.g. MessagePort used by React 19's scheduler) call internally.
    tokio::task::LocalSet::new().block_on(&rt, async move {
        // Synthetic URL — only required as metadata for MainWorker bootstrap.
        // All bundles are loaded via execute_script, not ES module resolution.
        let main_module_url = match Url::parse("https://ssr-deno.local/") {
            Ok(url) => url,
            Err(e) => {
                let _ = init_tx.send(Err(format!("Cannot build worker URL: {e}")));
                return;
            }
        };

        let mut worker = match build_worker(&main_module_url, max_heap_size_mb) {
            Ok(w) => w,
            Err(e) => {
                let _ = init_tx.send(Err(e));
                return;
            }
        };

        let _ = init_tx.send(Ok(()));

        while let Some(msg) = rx.recv().await {
            match msg {
                WorkerMsg::LoadBundle {
                    bundle_id,
                    bundle_code,
                    script_name,
                    reply,
                } => {
                    let result =
                        load_bundle_in_worker(&mut worker, &bundle_id, bundle_code, script_name);
                    let _ = reply.send(result);
                }
                WorkerMsg::Render {
                    bundle_id,
                    args_json,
                    reply,
                } => {
                    let result = call_render(&mut worker, &bundle_id, &args_json);
                    let _ = reply.send(result);
                }
            }
        }
    });
}

/// Evaluates the bundle code and moves `globalThis.render` into the bundle
/// namespace: `globalThis.__ssr_bundles[bundle_id] = { render: globalThis.render }`.
fn load_bundle_in_worker(
    worker: &mut MainWorker,
    bundle_id: &str,
    bundle_code: String,
    script_name: &'static str,
) -> Result<(), String> {
    if let Err(e) = worker.execute_script(script_name, bundle_code.into()) {
        return Err(format!("Failed to evaluate SSR bundle: {e}"));
    }

    // Move globalThis.render into the bundle namespace so multiple bundles
    // can coexist in the same V8 context without overwriting each other.
    // bundle_id is validated to [a-zA-Z0-9_-] before reaching here.
    let namespace_script = format!(
        r#"(function(id) {{
            if (typeof globalThis.__ssr_bundles === 'undefined') {{
                globalThis.__ssr_bundles = {{}};
            }}
            if (typeof globalThis.render !== 'function') {{
                throw new Error('Bundle did not assign a function to globalThis.render');
            }}
            globalThis.__ssr_bundles[id] = {{ render: globalThis.render }};
            globalThis.render = undefined;
        }})("{bundle_id}");"#
    );

    worker
        .execute_script("<ssr-deno:namespace>", namespace_script.into())
        .map(|_| ())
        .map_err(|e| format!("Failed to namespace bundle '{bundle_id}': {e}"))
}

fn build_worker(main_module: &Url, max_heap_size_mb: usize) -> Result<MainWorker, String> {
    let services = WorkerServiceOptions {
        blob_store: Arc::new(deno_runtime::deno_web::BlobStore::default()),
        broadcast_channel: Default::default(),
        deno_rt_native_addon_loader: None,
        feature_checker: Arc::new(FeatureChecker::default()),
        fs: Arc::new(deno_runtime::deno_fs::RealFs),
        module_loader: std::rc::Rc::new(deno_runtime::deno_core::NoopModuleLoader),
        node_services: None,
        npm_process_state_provider: None,
        permissions: PermissionsContainer::new(
            Arc::new(NopPermissionDescriptorParser),
            Permissions::none_without_prompt(),
        ),
        root_cert_store_provider: None,
        fetch_dns_resolver: Default::default(),
        shared_array_buffer_store: None,
        compiled_wasm_module_store: None,
        v8_code_cache: None,
        bundle_provider: None,
    };

    // Apply optional V8 heap size limit. When set (> 0), V8 will not exceed
    // this cap for the old generation. When 0, no CreateParams is passed and
    // V8 uses its built-in default (~1.4 GB on 64-bit).
    let create_params = if max_heap_size_mb > 0 {
        Some(
            v8::CreateParams::default()
                .set_max_old_generation_size_in_bytes(max_heap_size_mb * 1024 * 1024),
        )
    } else {
        None
    };

    let options = WorkerOptions {
        bootstrap: BootstrapOptions::default(),
        extensions: vec![],
        startup_snapshot: None,
        skip_op_registration: false,
        create_params,
        unsafely_ignore_certificate_errors: None,
        seed: None,
        create_web_worker_cb: Arc::new(|_| unimplemented!("web workers are not supported")),
        format_js_error_fn: None,
        should_break_on_first_statement: false,
        should_wait_for_inspector_session: false,
        trace_ops: None,
        cache_storage_dir: None,
        origin_storage_dir: None,
        stdio: Default::default(),
        enable_raw_imports: false,
        enable_stack_trace_arg_in_ops: false,
        unconfigured_runtime: None,
    };

    Ok(MainWorker::bootstrap_from_options::<
        NopInNpmPackageChecker,
        NopNpmPackageFolderResolver,
        Sys,
    >(main_module, services, options))
}

// ---------------------------------------------------------------------------
// V8 render call
// ---------------------------------------------------------------------------

fn call_render(
    worker: &mut MainWorker,
    bundle_id: &str,
    args_json: &str,
) -> Result<String, DenoError> {
    let js_runtime = &mut worker.js_runtime;
    let context = js_runtime.main_context();
    let isolate = js_runtime.v8_isolate();

    let scope_storage = std::pin::pin!(v8::HandleScope::new(isolate));
    let mut scope = scope_storage.init();
    let context_local = v8::Local::new(&mut scope, context);
    let mut context_scope = v8::ContextScope::new(&mut scope, context_local);

    let global = context_local.global(&mut context_scope);

    // globalThis.__ssr_bundles
    let bundles_key = v8::String::new(&mut context_scope, "__ssr_bundles").unwrap();
    let bundles_val = global
        .get(&mut context_scope, bundles_key.into())
        .filter(|v| !v.is_undefined() && !v.is_null())
        .ok_or_else(|| DenoError::BundleNotFound(format!("No bundles loaded (id: {bundle_id})")))?;

    let bundles_obj: v8::Local<v8::Object> = bundles_val.try_into().map_err(|_| {
        DenoError::BundleNotFound(format!("__ssr_bundles is not an object (id: {bundle_id})"))
    })?;

    // globalThis.__ssr_bundles[bundle_id]
    let id_key = v8::String::new(&mut context_scope, bundle_id).unwrap();
    let entry_val = bundles_obj
        .get(&mut context_scope, id_key.into())
        .filter(|v| !v.is_undefined() && !v.is_null())
        .ok_or_else(|| DenoError::BundleNotFound(format!("Bundle '{bundle_id}' not found")))?;

    let entry_obj: v8::Local<v8::Object> = entry_val.try_into().map_err(|_| {
        DenoError::BundleNotFound(format!("Bundle '{bundle_id}' entry is not an object"))
    })?;

    // globalThis.__ssr_bundles[bundle_id].render
    let render_key = v8::String::new(&mut context_scope, "render").unwrap();
    let render_val = entry_obj
        .get(&mut context_scope, render_key.into())
        .filter(|v| !v.is_undefined() && !v.is_null())
        .ok_or_else(|| {
            DenoError::BundleNotFound(format!("Bundle '{bundle_id}' has no render function"))
        })?;

    let render_fn: v8::Local<v8::Function> = render_val.try_into().map_err(|_| {
        DenoError::BundleNotFound(format!("Bundle '{bundle_id}' render is not a function"))
    })?;

    let args_v8 = v8::String::new(&mut context_scope, args_json).unwrap();
    let undefined = v8::undefined(&mut context_scope);

    // TryCatch prevents V8 from marking the exception as unhandled, which would
    // cause Deno's event loop to print "Uncaught ..." to stderr on the next tick.
    let tc = std::pin::pin!(v8::TryCatch::new(&mut context_scope));
    let try_catch = tc.init();

    let call_result = render_fn.call(&try_catch, undefined.into(), &[args_v8.into()]);

    let result = match call_result {
        Some(v) => v,
        None => {
            let msg = try_catch
                .message()
                .map(|m| m.get(&try_catch).to_rust_string_lossy(&try_catch))
                .unwrap_or_else(|| "`render` function threw an exception".to_string());
            return Err(DenoError::Render(msg));
        }
    };

    // JSON-serialize so any JS type (string, object, array, …) survives the
    // V8→Rust→Ruby boundary. Ruby's JSON.parse reconstructs the value.
    let json_str = v8::json::stringify(&try_catch, result)
        .ok_or_else(|| DenoError::Render("Cannot serialize render result to JSON".to_string()))?;

    Ok(json_str.to_rust_string_lossy(&try_catch))
}
