use std::borrow::Cow;
use std::path::Path;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use deno_resolver::npm::{ByonmInNpmPackageChecker, ByonmNpmResolver};
use deno_runtime::deno_core::url::Url;
use deno_runtime::deno_core::v8;
use deno_runtime::deno_fs::sync::MaybeArc;
use deno_runtime::deno_node::{NodeExtInitServices, NodeRequireLoaderRc, NodeResolver};
use deno_runtime::deno_permissions::{
    Permissions, PermissionsContainer, PermissionsOptions, RuntimePermissionDescriptorParser,
};
use deno_runtime::worker::{MainWorker, WorkerOptions, WorkerServiceOptions};
use deno_runtime::BootstrapOptions;
use deno_runtime::FeatureChecker;
use node_resolver::cache::NodeResolutionSys;
use node_resolver::{DenoIsBuiltInNodeModuleChecker, NodeConditionOptions, NodeResolverOptions};

use crate::dev_mode_module_loader::{
    DevModeModuleLoader, DevModeMtimeCache, SharedAliasMap, SharedCjsPaths,
};
use crate::dev_mode_npm_resolver::build_dev_mode_npm_resolver;
use crate::require_loader::DevModeNodeRequireLoader;
use ssr_deno_sys::Sys;

type DevNodeServices = NodeExtInitServices<ByonmInNpmPackageChecker, ByonmNpmResolver<Sys>, Sys>;

fn build_dev_node_services(
    npm_checker: ByonmInNpmPackageChecker,
    npm_resolver: ByonmNpmResolver<Sys>,
    pkg_json_resolver: node_resolver::PackageJsonResolverRc<Sys>,
) -> Option<DevNodeServices> {
    let loader: NodeRequireLoaderRc = Rc::new(DevModeNodeRequireLoader);

    let resolver: MaybeArc<NodeResolver<ByonmInNpmPackageChecker, ByonmNpmResolver<Sys>, Sys>> = {
        let r = NodeResolver::new(
            npm_checker,
            DenoIsBuiltInNodeModuleChecker,
            npm_resolver,
            pkg_json_resolver.clone(),
            NodeResolutionSys::new(Sys, None),
            NodeResolverOptions {
                // Base conditions used when no context is overridden.
                // Context-specific overrides below match Node's defaults:
                // `import` for ESM, `require` for CJS. Without the
                // `require_conditions_override`, deno_node's `createRequire`
                // would resolve npm packages under `["node","import"]`,
                // which picks the `.cjs.mjs` ESM-wrapper for emotion/MUI
                // packages. Node then refuses `require()` of ESM in a
                // cycle. Splitting the overrides routes `require()` calls
                // to the `.cjs.js` files directly.
                conditions: NodeConditionOptions {
                    conditions: vec![Cow::Borrowed("node")],
                    import_conditions_override: Some(vec![
                        Cow::Borrowed("node"),
                        Cow::Borrowed("import"),
                    ]),
                    require_conditions_override: Some(vec![
                        Cow::Borrowed("node"),
                        Cow::Borrowed("require"),
                    ]),
                },
                is_browser_platform: false,
                bundle_mode: true,
                typescript_version: None,
            },
        );
        MaybeArc::new(r)
    };

    Some(DevNodeServices {
        node_require_loader: loader,
        node_resolver: resolver,
        pkg_json_resolver,
        sys: Sys,
    })
}

pub fn build_dev_mode_worker(
    main_module: &Url,
    max_heap_size_mb: usize,
    resolve_aliases: SharedAliasMap,
    project_root: &Path,
    oom_triggered: Arc<AtomicBool>,
    mtime_cache: Arc<DevModeMtimeCache>,
    cjs_paths: SharedCjsPaths,
) -> Result<MainWorker, String> {
    let (npm_checker, npm_resolver, pkg_json_resolver) = build_dev_mode_npm_resolver(project_root);

    let node_services = build_dev_node_services(npm_checker, npm_resolver, pkg_json_resolver);

    let module_loader: Rc<dyn deno_runtime::deno_core::ModuleLoader> = {
        let loader = DevModeModuleLoader::new(
            project_root.to_path_buf(),
            resolve_aliases,
            mtime_cache,
            cjs_paths,
        );
        Rc::new(loader)
    };

    let perms_parser = Arc::new(RuntimePermissionDescriptorParser::new(Sys));
    let perms_opts = PermissionsOptions {
        allow_read: Some(vec![project_root.to_string_lossy().into_owned()]),
        // `Some(vec![])` = grant globally (per `global_from_option` in
        // deno_permissions). Required because every CJS-shaped npm package
        // boots through a `process.env.NODE_ENV` branch (React, MUI,
        // emotion, scheduler …); without env access the require() throws
        // before exports are assigned. Reads are non-sensitive.
        allow_env: Some(vec![]),
        // `os.platform()` / `os.arch()` lookups inside some npm transitive
        // deps (cross-spawn, etc.) need sys access to avoid hard-erroring.
        allow_sys: Some(vec![]),
        prompt: false,
        ..Default::default()
    };
    let perms = Permissions::from_options(perms_parser.as_ref(), &perms_opts)
        .map_err(|e| format!("Permissions::from_options: {e}"))?;

    let services = WorkerServiceOptions {
        blob_store: Arc::new(deno_runtime::deno_web::BlobStore::default()),
        broadcast_channel: Default::default(),
        deno_rt_native_addon_loader: None,
        feature_checker: Arc::new(FeatureChecker::default()),
        fs: Arc::new(deno_runtime::deno_fs::RealFs),
        module_loader,
        node_services,
        npm_process_state_provider: None,
        permissions: PermissionsContainer::new(perms_parser, perms),
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
        ByonmInNpmPackageChecker,
        ByonmNpmResolver<Sys>,
        Sys,
    >(main_module, services, options);

    let isolate_handle = worker.js_runtime.v8_isolate().thread_safe_handle();
    worker
        .js_runtime
        .add_near_heap_limit_callback(move |current_limit, _initial_limit| {
            oom_triggered.store(true, Ordering::SeqCst);
            let _ = isolate_handle.terminate_execution();
            current_limit * 2
        });

    Ok(worker)
}
