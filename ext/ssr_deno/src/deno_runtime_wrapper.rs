use std::sync::mpsc;
use std::sync::Arc;

use deno_runtime::deno_core::url::Url;
use deno_runtime::deno_core::v8;
use deno_runtime::deno_permissions::Permissions;
use deno_runtime::deno_permissions::PermissionsContainer;
use deno_runtime::worker::MainWorker;
use deno_runtime::worker::WorkerOptions;
use deno_runtime::worker::WorkerServiceOptions;
use deno_runtime::BootstrapOptions;
use deno_runtime::FeatureChecker;

use crate::nop_types::NopPermissionDescriptorParser;
use crate::nop_types::NopInNpmPackageChecker;
use crate::nop_types::NopNpmPackageFolderResolver;
use crate::sys::Sys;

// ---------------------------------------------------------------------------
// Typed error enum
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum DenoError {
    BundleLoad(String),
    WorkerInit(String),
    WorkerDied(String),
    BundleNotFound(String),
    Render(String),
}

impl std::fmt::Display for DenoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BundleLoad(msg)
            | Self::WorkerInit(msg)
            | Self::WorkerDied(msg)
            | Self::BundleNotFound(msg)
            | Self::Render(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for DenoError {}

// ---------------------------------------------------------------------------
// Wire protocol between the Ruby thread and the Deno worker thread
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
        fn_name: String,
        args_json: String,
        reply: tokio::sync::oneshot::Sender<Result<String, DenoError>>,
    },
}

// ---------------------------------------------------------------------------
// DenoRuntimeWrapper
// ---------------------------------------------------------------------------

/// Owns the channel to a dedicated background thread that runs the Deno
/// `MainWorker` (V8 isolate + Web API extensions).
///
/// Because `MainWorker` never leaves its thread, no `unsafe` impl or
/// `UnsafeCell` is required — `tokio::sync::mpsc::Sender` is `Send + Sync`
/// on its own.
pub struct DenoRuntimeWrapper {
    tx: tokio::sync::mpsc::Sender<WorkerMsg>,
}

impl DenoRuntimeWrapper {
    /// Spawns the Deno worker thread and blocks until it is ready to accept
    /// bundle-load and render requests. No bundle is evaluated at this stage.
    pub fn new() -> Result<Self, DenoError> {
        let (tx, rx) = tokio::sync::mpsc::channel::<WorkerMsg>(1);
        let (init_tx, init_rx) = mpsc::sync_channel::<Result<(), String>>(1);

        std::thread::Builder::new()
            .name("deno-worker".into())
            .spawn(move || worker_thread_main(rx, init_tx))
            .map_err(|e| DenoError::WorkerInit(format!("Failed to spawn worker thread: {e}")))?;

        init_rx
            .recv()
            .map_err(|_| DenoError::WorkerInit("Deno worker thread exited unexpectedly during init".into()))?
            .map_err(DenoError::WorkerInit)?;

        Ok(Self { tx })
    }

    /// Evaluates a Vite SSR bundle and registers its exported functions under
    /// `globalThis.__ssr_bundles[bundle_id]`. Safe to call for multiple bundles.
    pub fn load_bundle(&self, bundle_id: &str, bundle_path: &str) -> Result<(), DenoError> {
        let bundle_name = std::path::Path::new(bundle_path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("(unknown)");
        let canonical = std::fs::canonicalize(bundle_path)
            .map_err(|e| DenoError::BundleLoad(format!("Cannot resolve bundle path '{bundle_name}': {e}")))?;

        // Reject symlink escapes: the resolved path must stay within the
        // directory that was originally specified (e.g. entry.js -> /etc/secret
        // would escape /app/dist/ and be caught here).
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

        let bundle_code = std::fs::read_to_string(bundle_path)
            .map_err(|e| DenoError::BundleLoad(format!("Cannot read bundle file '{bundle_name}': {e}")))?;

        // `MainWorker::execute_script` requires `&'static str` for the script
        // name. One bounded leak per bundle load (process-lifetime here).
        let script_name: &'static str = canonical
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| Box::leak(s.to_owned().into_boxed_str()) as &'static str)
            .unwrap_or("main.js");

        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();

        self.tx
            .blocking_send(WorkerMsg::LoadBundle {
                bundle_id: bundle_id.to_string(),
                bundle_code,
                script_name,
                reply: reply_tx,
            })
            .map_err(|_| DenoError::WorkerDied("Deno worker thread has exited".into()))?;

        reply_rx
            .blocking_recv()
            .map_err(|_| DenoError::WorkerDied("Deno worker thread exited before sending a reply".into()))?
            .map_err(DenoError::BundleLoad)
    }

    /// Sends a render request to the worker thread and blocks until the result
    /// arrives. Safe to call from a non-async context (e.g. Ruby's GVL thread).
    /// Returns the result as a JSON string so any JS type survives the boundary.
    pub fn block_on_render(&self, bundle_id: &str, fn_name: &str, args_json: &str) -> Result<String, DenoError> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();

        self.tx
            .blocking_send(WorkerMsg::Render {
                bundle_id: bundle_id.to_string(),
                fn_name: fn_name.to_string(),
                args_json: args_json.to_string(),
                reply: reply_tx,
            })
            .map_err(|_| DenoError::WorkerDied("Deno worker thread has exited".into()))?;

        reply_rx
            .blocking_recv()
            .map_err(|_| DenoError::WorkerDied("Deno worker thread exited before sending a reply".into()))?
    }
}

// ---------------------------------------------------------------------------
// Worker thread
// ---------------------------------------------------------------------------

fn worker_thread_main(
    mut rx: tokio::sync::mpsc::Receiver<WorkerMsg>,
    init_tx: mpsc::SyncSender<Result<(), String>>,
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

        let mut worker = match build_worker(&main_module_url) {
            Ok(w) => w,
            Err(e) => {
                let _ = init_tx.send(Err(e));
                return;
            }
        };

        let _ = init_tx.send(Ok(()));

        while let Some(msg) = rx.recv().await {
            match msg {
                WorkerMsg::LoadBundle { bundle_id, bundle_code, script_name, reply } => {
                    let result = load_bundle_in_worker(&mut worker, &bundle_id, bundle_code, script_name);
                    let _ = reply.send(result);
                }
                WorkerMsg::Render { bundle_id, fn_name, args_json, reply } => {
                    let result = call_render(&mut worker, &bundle_id, &fn_name, &args_json);
                    let _ = reply.send(result);
                }
            }
        }
    });
}

/// Evaluates the bundle code and registers its exported functions under
/// `globalThis.__ssr_bundles[bundle_id]`.
///
/// The bundle is wrapped in an IIFE so top-level `function` and `var`
/// declarations stay IIFE-local and do not pollute globalThis. Only explicit
/// `globalThis.name = fn` assignments inside the bundle escape the scope,
/// creating new configurable, deletable properties — exactly the intended
/// exports. A name-snapshot taken before eval identifies those new properties;
/// each is captured into the bundle namespace and deleted from globalThis so
/// the next bundle load sees a clean baseline.
fn load_bundle_in_worker(
    worker: &mut MainWorker,
    bundle_id: &str,
    bundle_code: String,
    script_name: &'static str,
) -> Result<(), String> {
    let snapshot_script = r#"(function() {
        globalThis.__ssr_snapshot = new Set(Object.getOwnPropertyNames(globalThis));
        globalThis.__ssr_snapshot.add('__ssr_snapshot');
    })();"#;

    if let Err(e) = worker.execute_script("<ssr-deno:snapshot>", snapshot_script.to_string().into()) {
        return Err(format!("Failed to snapshot globalThis: {e}"));
    }

    // IIFE wrapper keeps all function/var declarations local; only
    // `globalThis.name = fn` assignments reach the global object.
    let wrapped_code = format!("(function(){{\n{bundle_code}\n}})();");
    if let Err(e) = worker.execute_script(script_name, wrapped_code.into()) {
        return Err(format!("Failed to evaluate SSR bundle: {e}"));
    }

    // Every new function property is a bundle export (the IIFE ensures no
    // accidental leakage from declarations). Delete after capture so the next
    // bundle load starts with a clean snapshot.
    // bundle_id is numeric (Ruby object_id) so interpolation is safe.
    let namespace_script = format!(
        r#"(function(id) {{
            if (typeof globalThis.__ssr_bundles === 'undefined') {{
                globalThis.__ssr_bundles = {{}};
            }}
            var snap = globalThis.__ssr_snapshot;
            var ns = {{}};
            var found = false;
            for (var key of Object.getOwnPropertyNames(globalThis)) {{
                if (!snap.has(key) && typeof globalThis[key] === 'function') {{
                    ns[key] = globalThis[key];
                    delete globalThis[key];
                    found = true;
                }}
            }}
            if (!found) {{
                throw new Error('Bundle did not assign any functions to globalThis');
            }}
            globalThis.__ssr_bundles[id] = ns;
            globalThis.__ssr_snapshot = undefined;
        }})("{bundle_id}");"#
    );

    worker
        .execute_script("<ssr-deno:namespace>", namespace_script.into())
        .map(|_| ())
        .map_err(|e| format!("Failed to namespace bundle '{bundle_id}': {e}"))
}

fn build_worker(main_module: &Url) -> Result<MainWorker, String> {
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

    let options = WorkerOptions {
        bootstrap: BootstrapOptions::default(),
        extensions: vec![],
        startup_snapshot: None,
        skip_op_registration: false,
        create_params: None,
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

fn call_render(worker: &mut MainWorker, bundle_id: &str, fn_name: &str, args_json: &str) -> Result<String, DenoError> {
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

    let bundles_obj: v8::Local<v8::Object> = bundles_val
        .try_into()
        .map_err(|_| DenoError::BundleNotFound(format!("__ssr_bundles is not an object (id: {bundle_id})")))?;

    // globalThis.__ssr_bundles[bundle_id]
    let id_key = v8::String::new(&mut context_scope, bundle_id).unwrap();
    let entry_val = bundles_obj
        .get(&mut context_scope, id_key.into())
        .filter(|v| !v.is_undefined() && !v.is_null())
        .ok_or_else(|| DenoError::BundleNotFound(format!("Bundle '{bundle_id}' not found")))?;

    let entry_obj: v8::Local<v8::Object> = entry_val
        .try_into()
        .map_err(|_| DenoError::BundleNotFound(format!("Bundle '{bundle_id}' entry is not an object")))?;

    // globalThis.__ssr_bundles[bundle_id][fn_name]
    let fn_key = v8::String::new(&mut context_scope, fn_name).unwrap();
    let render_val = entry_obj
        .get(&mut context_scope, fn_key.into())
        .filter(|v| !v.is_undefined() && !v.is_null())
        .ok_or_else(|| DenoError::BundleNotFound(format!("Bundle '{bundle_id}' has no function '{fn_name}'")))?;

    let render_fn: v8::Local<v8::Function> = render_val
        .try_into()
        .map_err(|_| DenoError::BundleNotFound(format!("Bundle '{bundle_id}' '{fn_name}' is not a function")))?;

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
                .unwrap_or_else(|| format!("'{fn_name}' function threw an exception"));
            return Err(DenoError::Render(msg));
        }
    };

    // JSON-serialize so any JS type (string, object, array, …) survives the
    // V8→Rust→Ruby boundary. Ruby's JSON.parse reconstructs the value.
    let json_str = v8::json::stringify(&try_catch, result)
        .ok_or_else(|| DenoError::Render("Cannot serialize render result to JSON".to_string()))?;

    Ok(json_str.to_rust_string_lossy(&try_catch))
}
