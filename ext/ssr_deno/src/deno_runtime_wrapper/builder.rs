use std::borrow::Cow;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use deno_runtime::deno_core::url::Url;
use deno_runtime::deno_core::v8;
use deno_runtime::deno_fs::sync::MaybeArc;
use deno_runtime::deno_node::{NodeExtInitServices, NodeRequireLoaderRc, NodeResolver};
use deno_runtime::deno_permissions::{Permissions, PermissionsContainer};
use deno_runtime::worker::{MainWorker, WorkerOptions, WorkerServiceOptions};
use deno_runtime::BootstrapOptions;
use deno_runtime::FeatureChecker;
use node_resolver::cache::NodeResolutionSys;
use node_resolver::DenoIsBuiltInNodeModuleChecker;
use node_resolver::{NodeConditionOptions, NodeResolverOptions, PackageJsonResolver};

use crate::node_builtin_loader::NodeBuiltinOnlyModuleLoader;
use crate::nop_types::{
    NopInNpmPackageChecker, NopNpmPackageFolderResolver, NopPermissionDescriptorParser,
};
use crate::require_loader::SSRDenoNodeRequireLoader;
use crate::sys::Sys;

// ---------------------------------------------------------------------------
// build_worker — broken into focused helpers
// ---------------------------------------------------------------------------

type NodeServices = NodeExtInitServices<NopInNpmPackageChecker, NopNpmPackageFolderResolver, Sys>;

/// Constructs `NodeExtInitServices` for the `deno_node` extension when
/// `node_builtins` is enabled. Returns `None` otherwise.
fn build_node_services(node_builtins: bool) -> Option<NodeServices> {
    if !node_builtins {
        return None;
    }

    let loader: NodeRequireLoaderRc = Rc::new(SSRDenoNodeRequireLoader);

    let pkg_json_resolver = MaybeArc::new(PackageJsonResolver::new(Sys, None));

    let resolver: MaybeArc<NodeResolver<NopInNpmPackageChecker, NopNpmPackageFolderResolver, Sys>> = {
        let r = NodeResolver::new(
            NopInNpmPackageChecker,
            DenoIsBuiltInNodeModuleChecker,
            NopNpmPackageFolderResolver,
            pkg_json_resolver.clone(),
            NodeResolutionSys::new(Sys, None),
            NodeResolverOptions {
                conditions: NodeConditionOptions {
                    conditions: vec![Cow::Borrowed("node"), Cow::Borrowed("import")],
                    import_conditions_override: None,
                    require_conditions_override: None,
                },
                is_browser_platform: false,
                bundle_mode: true,
                typescript_version: None,
            },
        );
        MaybeArc::new(r)
    };

    Some(NodeExtInitServices {
        node_require_loader: loader,
        node_resolver: resolver,
        pkg_json_resolver,
        sys: Sys,
    })
}

pub fn build_worker(
    main_module: &Url,
    max_heap_size_mb: usize,
    node_builtins: bool,
    oom_triggered: Arc<AtomicBool>,
) -> Result<MainWorker, String> {
    let node_services = build_node_services(node_builtins);

    let module_loader: Rc<dyn deno_runtime::deno_core::ModuleLoader> = if node_builtins {
        Rc::new(NodeBuiltinOnlyModuleLoader)
    } else {
        Rc::new(deno_runtime::deno_core::NoopModuleLoader)
    };

    let services = WorkerServiceOptions {
        blob_store: Arc::new(deno_runtime::deno_web::BlobStore::default()),
        broadcast_channel: Default::default(),
        deno_rt_native_addon_loader: None,
        feature_checker: Arc::new(FeatureChecker::default()),
        fs: Arc::new(deno_runtime::deno_fs::RealFs),
        module_loader,
        node_services,
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
        extensions: vec![deno_runtime::deno_core::Extension {
            name: "ssr_deno_ops",
            ops: Cow::Owned(vec![]),
            ..Default::default()
        }],
        startup_snapshot: None,
        skip_op_registration: false,
        create_params,
        unsafely_ignore_certificate_errors: None,
        seed: None,
        // Web Workers are not supported. If JS calls `new Worker()`, this
        // callback panics inside the worker thread (the V8 isolate thread
        // spawned in IsolateHandle::spawn). The panic is CONTAINED to that
        // thread — the worker dies, the reply channel drops, and the main
        // Ruby thread gets `blocking_recv() -> Err` → `JsRuntimeWorkerError`.
        // No undefined behavior crosses the FFI boundary because the panic
        // happens on a separate OS thread. See test:
        // `test_web_worker_in_ssr_bundle_does_not_crash_process`.
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

    let mut worker = MainWorker::bootstrap_from_options::<
        NopInNpmPackageChecker,
        NopNpmPackageFolderResolver,
        Sys,
    >(main_module, services, options);

    let isolate_handle = worker.js_runtime.v8_isolate().thread_safe_handle();
    // `current_limit * 2` is the standard V8 heap-growth pattern — terminate
    // execution immediately and double the limit so V8 can unwind gracefully.
    // Deno's own tests use the same formula. The doubled limit persists across
    // renders, but in practice OOM is a single-shot event per render (execution
    // is terminated). No cap needed — pathological repeated OOM on the same
    // isolate would require hundreds of renders each hitting a progressively
    // higher limit, which doesn't occur in practice.
    worker
        .js_runtime
        .add_near_heap_limit_callback(move |current_limit, _initial_limit| {
            oom_triggered.store(true, Ordering::SeqCst);
            let _ = isolate_handle.terminate_execution();
            current_limit * 2
        });

    Ok(worker)
}
