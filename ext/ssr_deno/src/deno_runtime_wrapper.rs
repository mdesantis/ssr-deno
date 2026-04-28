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
// Wire protocol between the Ruby thread and the Deno worker thread
// ---------------------------------------------------------------------------

enum WorkerMsg {
    Render {
        args_json: String,
        reply: tokio::sync::oneshot::Sender<Result<String, String>>,
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
    /// Spawns the Deno worker thread, evaluates the SSR bundle, and blocks
    /// until the worker signals that initialization is complete.
    pub fn new(bundle_path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        // Validate and read eagerly so errors surface in the caller's context,
        // not silently inside the background thread.
        let canonical = std::fs::canonicalize(bundle_path)
            .map_err(|e| format!("Cannot resolve bundle path '{bundle_path}': {e}"))?;
        let bundle_code = std::fs::read_to_string(bundle_path)
            .map_err(|e| format!("Cannot read bundle file '{bundle_path}': {e}"))?;

        // `MainWorker::execute_script` requires `&'static str` for the script
        // name. One bounded leak per wrapper instance (process-lifetime here).
        let script_name: &'static str = canonical
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| Box::leak(s.to_owned().into_boxed_str()) as &'static str)
            .unwrap_or("main.js");

        let (tx, rx) = tokio::sync::mpsc::channel::<WorkerMsg>(1);
        let (init_tx, init_rx) = mpsc::sync_channel::<Result<(), String>>(1);

        std::thread::Builder::new()
            .name("deno-worker".into())
            .spawn(move || worker_thread_main(canonical, bundle_code, script_name, rx, init_tx))?;

        init_rx
            .recv()
            .map_err(|_| "Deno worker thread exited unexpectedly during init")?
            .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;

        Ok(Self { tx })
    }

    /// Sends a render request to the worker thread and blocks until the result
    /// arrives. Safe to call from a non-async context (e.g. Ruby's GVL thread).
    pub fn block_on_render(&self, args_json: &str) -> Result<String, Box<dyn std::error::Error>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();

        self.tx
            .blocking_send(WorkerMsg::Render {
                args_json: args_json.to_string(),
                reply: reply_tx,
            })
            .map_err(|_| "Deno worker thread has exited")?;

        reply_rx
            .blocking_recv()
            .map_err(|_| "Deno worker thread exited before sending a reply")?
            .map_err(Into::into)
    }
}

// ---------------------------------------------------------------------------
// Worker thread
// ---------------------------------------------------------------------------

fn worker_thread_main(
    main_module_path: std::path::PathBuf,
    bundle_code: String,
    script_name: &'static str,
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
        let main_module_url = match Url::from_file_path(&main_module_path) {
            Ok(url) => url,
            Err(_) => {
                let _ = init_tx.send(Err(format!(
                    "Cannot convert path to URL: {}",
                    main_module_path.display()
                )));
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

        if let Err(e) = worker.execute_script(script_name, bundle_code.into()) {
            let _ = init_tx.send(Err(format!("Failed to evaluate SSR bundle: {e}")));
            return;
        }

        let _ = init_tx.send(Ok(()));

        while let Some(msg) = rx.recv().await {
            match msg {
                WorkerMsg::Render { args_json, reply } => {
                    let result = call_render(&mut worker, &args_json).map_err(|e| e.to_string());
                    let _ = reply.send(result);
                }
            }
        }
    });
}

fn build_worker(main_module: &Url) -> Result<MainWorker, String> {
    let services = WorkerServiceOptions {
        blob_store: Arc::new(deno_runtime::deno_web::BlobStore::default()),
        broadcast_channel: Default::default(),
        deno_rt_native_addon_loader: None,
        feature_checker: Arc::new(FeatureChecker::default()),
        fs: Arc::new(deno_runtime::deno_fs::RealFs),
        module_loader: std::rc::Rc::new(deno_runtime::deno_core::FsModuleLoader),
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

fn call_render(worker: &mut MainWorker, args_json: &str) -> Result<String, Box<dyn std::error::Error>> {
    let js_runtime = &mut worker.js_runtime;
    let context = js_runtime.main_context();
    let isolate = js_runtime.v8_isolate();

    let scope_storage = std::pin::pin!(v8::HandleScope::new(isolate));
    let mut scope = scope_storage.init();
    let context_local = v8::Local::new(&mut scope, context);
    let mut context_scope = v8::ContextScope::new(&mut scope, context_local);

    let global = context_local.global(&mut context_scope);

    let render_key = v8::String::new(&mut context_scope, "render").unwrap();
    let render_val = global
        .get(&mut context_scope, render_key.into())
        .ok_or("`render` is not defined in the global scope")?;

    let render_fn: v8::Local<v8::Function> = render_val
        .try_into()
        .map_err(|_| "`render` is not a function")?;

    let args_v8 = v8::String::new(&mut context_scope, args_json).unwrap();
    let undefined = v8::undefined(&mut context_scope);

    let result = render_fn
        .call(&mut context_scope, undefined.into(), &[args_v8.into()])
        .ok_or("`render` function threw an exception")?;

    let result_str = result
        .to_string(&mut context_scope)
        .ok_or("Cannot convert render result to string")?;

    Ok(result_str.to_rust_string_lossy(&context_scope))
}
