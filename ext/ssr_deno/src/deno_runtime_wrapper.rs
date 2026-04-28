use std::cell::UnsafeCell;
use std::sync::Arc;

use deno_runtime::deno_core::url::Url;
use deno_runtime::deno_core::v8;
use deno_runtime::deno_permissions::PermissionsContainer;
use deno_runtime::worker::MainWorker;
use deno_runtime::worker::WorkerOptions;
use deno_runtime::worker::WorkerServiceOptions;
use deno_runtime::BootstrapOptions;
use deno_runtime::FeatureChecker;

use crate::nop_types::AllowAllPermissionDescriptorParser;
use crate::nop_types::NopInNpmPackageChecker;
use crate::nop_types::NopNpmPackageFolderResolver;
use crate::sys::Sys;

// ---------------------------------------------------------------------------
// DenoRuntimeWrapper
// ---------------------------------------------------------------------------

/// Wraps a Tokio runtime and a `deno_runtime::MainWorker` (V8 isolate with Deno
/// Web API extensions) for SSR.
///
/// The Vite SSR bundle is loaded and evaluated once at initialization.
/// Each call to `block_on_render` extracts the `render` function from the
/// V8 global scope, calls it with JSON-serialized arguments, and returns
/// the rendered HTML string.
///
/// # Web API Support
///
/// `MainWorker` provides all Deno Web API extensions out of the box:
/// - `MessageChannel` / `MessagePort` (React 19 scheduler)
/// - `setTimeout` / `clearTimeout` / `setInterval` / `clearInterval`
/// - `performance.now()`
/// - `console`
/// - `TextEncoder` / `TextDecoder`
/// - `URL`, `Blob`, `FormData`, `Headers`
/// - `fetch`, `WebSocket`, `crypto`, and more
///
/// # Safety
///
/// `MainWorker` is not `Send` or `Sync` by default. However, since Ruby's GVL
/// ensures that only one thread accesses this struct at a time, it is safe to
/// implement these traits. The Tokio runtime is `Send` + `Sync`.
///
/// We use `UnsafeCell` for the `MainWorker` field to allow interior mutability
/// through an immutable reference. This is safe because Ruby's GVL serializes
/// all access, ensuring no concurrent mutable accesses occur.
pub struct DenoRuntimeWrapper {
    tokio_rt: tokio::runtime::Runtime,
    worker: UnsafeCell<MainWorker>,
}

// SAFETY: Ruby's GVL serializes all access to this struct. The Tokio runtime
// is only used for `block_on` which is called from the single Ruby thread.
unsafe impl Send for DenoRuntimeWrapper {}
unsafe impl Sync for DenoRuntimeWrapper {}

impl DenoRuntimeWrapper {
    /// Creates a new `DenoRuntimeWrapper`, loading and evaluating the SSR bundle.
    ///
    /// Initializes a `MainWorker` with all Deno Web API extensions and evaluates
    /// the self-contained Vite SSR bundle.
    ///
    /// # Arguments
    ///
    /// * `bundle_path` - Path to the self-contained Vite SSR bundle (entry-server.js)
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The bundle file cannot be read
    /// - The bundle JavaScript cannot be evaluated (syntax error, runtime error)
    pub fn new(bundle_path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        // Deno's internals (deno_unsync, spawn_local) require current_thread flavor.
        // A multi-threaded runtime triggers an assertion failure inside deno_unsync.
        let tokio_rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;

        // Resolve the bundle path to a file:// URL (the main module specifier).
        let main_module = Url::from_file_path(
            std::fs::canonicalize(bundle_path)
                .map_err(|e| format!("Cannot resolve bundle path '{bundle_path}': {e}"))?,
        )
        .map_err(|_| format!("Cannot convert bundle path to URL: {bundle_path}"))?;

        // -- Build WorkerServiceOptions --

        let module_loader = std::rc::Rc::new(deno_runtime::deno_core::FsModuleLoader);

        let permissions =
            PermissionsContainer::allow_all(Arc::new(AllowAllPermissionDescriptorParser));

        let services = WorkerServiceOptions {
            blob_store: Arc::new(deno_runtime::deno_web::BlobStore::default()),
            broadcast_channel: Default::default(),
            deno_rt_native_addon_loader: None,
            feature_checker: Arc::new(FeatureChecker::default()),
            fs: Arc::new(deno_runtime::deno_fs::RealFs),
            module_loader,
            node_services: None,
            npm_process_state_provider: None,
            permissions,
            root_cert_store_provider: None,
            fetch_dns_resolver: Default::default(),
            shared_array_buffer_store: None,
            compiled_wasm_module_store: None,
            v8_code_cache: None,
            bundle_provider: None,
        };

        // -- Build WorkerOptions --

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

        // -- Create the MainWorker --
        //
        // bootstrap_from_options internally calls tokio::spawn (e.g. for
        // SIGUSR2 signal handling and memory trimming), so we must enter the
        // Tokio runtime context first.
        let _enter = tokio_rt.enter();

        let mut worker = MainWorker::bootstrap_from_options::<
            NopInNpmPackageChecker,
            NopNpmPackageFolderResolver,
            Sys,
        >(&main_module, services, options);

        // -- Evaluate the bundle --

        let bundle_code = std::fs::read_to_string(bundle_path)
            .map_err(|e| format!("Cannot read bundle file '{bundle_path}': {e}"))?;

        // Use execute_script to evaluate the bundle in the global scope.
        // The bundle is expected to assign a `render` function to `globalThis`.
        worker
            .execute_script("entry-server.js", bundle_code.into())
            .map_err(|e| format!("Failed to evaluate SSR bundle: {e}"))?;

        Ok(Self {
            tokio_rt,
            worker: UnsafeCell::new(worker),
        })
    }

    /// Returns a mutable pointer to the inner `MainWorker`.
    ///
    /// # Safety
    ///
    /// Caller must ensure that no other mutable reference exists concurrently.
    /// Ruby's GVL guarantees this for single-threaded access.
    #[inline]
    fn worker_mut(&self) -> &mut MainWorker {
        // SAFETY: Ruby's GVL ensures single-threaded access, so getting a
        // mutable reference from an immutable one is safe here.
        unsafe { &mut *self.worker.get() }
    }

    /// Calls the `render` function from the evaluated SSR bundle with JSON args.
    ///
    /// # Arguments
    ///
    /// * `args_json` - JSON string containing `{ component_data, props, url }`
    ///
    /// # Returns
    ///
    /// The rendered HTML string from the SSR bundle.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The `render` function is not found in the global scope
    /// - The JavaScript `render` function throws an error
    /// - The V8 value cannot be converted to a string
    pub fn block_on_render(&self, args_json: &str) -> Result<String, Box<dyn std::error::Error>> {
        self.tokio_rt.block_on(async {
            let worker = self.worker_mut();
            let js_runtime = &mut worker.js_runtime;

            // Get the global object and look up the `render` function.
            let context = js_runtime.main_context();
            let isolate = js_runtime.v8_isolate();

            // Create a HandleScope using the pin!/init() pattern.
            // HandleScope::new(isolate) returns ScopeStorage<HandleScope<'_>>.
            // .init() transitions to PinnedRef<'_, HandleScope<'_>>.
            let scope_storage = std::pin::pin!(v8::HandleScope::new(isolate));
            let mut scope = scope_storage.init();

            // Enter the main context via ContextScope.
            // ContextScope wraps a PinnedRef<HandleScope> and implements DerefMut
            // to it, so &mut context_scope can be passed to V8 methods.
            let context_local = v8::Local::new(&mut scope, context);
            let mut context_scope = v8::ContextScope::new(&mut scope, context_local);

            let global = context_local.global(&mut context_scope);

            // Get the `render` function from globalThis
            let render_key = v8::String::new(&mut context_scope, "render").unwrap();
            let render_val = global.get(&mut context_scope, render_key.into());

            let render_val = render_val.ok_or("`render` is not defined in the global scope")?;
            let render_fn: v8::Local<v8::Function> = render_val
                .try_into()
                .map_err(|_| "`render` is not a function")?;

            // Create the argument (JSON string)
            let args_v8 = v8::String::new(&mut context_scope, args_json).unwrap();
            let undefined = v8::undefined(&mut context_scope);

            // Call the render function
            let result = render_fn
                .call(&mut context_scope, undefined.into(), &[args_v8.into()])
                .ok_or("`render` function threw an exception")?;

            // Convert the result to a Rust string
            let result_str = result
                .to_string(&mut context_scope)
                .ok_or("Cannot convert render result to string")?;
            Ok(result_str.to_rust_string_lossy(&context_scope))
        })
    }
}
