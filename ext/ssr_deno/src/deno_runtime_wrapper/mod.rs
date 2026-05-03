use std::borrow::Cow;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

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
use crate::node_builtin_loader::NodeBuiltinOnlyModuleLoader;
use crate::require_loader::DenoNodeRequireLoader;
use crate::sys::Sys;

pub use ssr_deno_core::DenoError;
pub use ssr_deno_core::{next_index, validate_pool_size};
// MAX_ISOLATES is available through ssr_deno_core::MAX_ISOLATES if needed.

pub(crate) mod call_render;
use self::call_render::{call_render, collect_heap_stats};

pub(crate) mod render_stream;

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
        render_timeout_ms: u64,
        reply: std::sync::mpsc::SyncSender<Result<String, DenoError>>,
    },
    HeapStats {
        reply: tokio::sync::oneshot::Sender<Result<String, DenoError>>,
    },
    RenderStream {
        bundle_id: String,
        args_json: String,
        render_timeout_ms: u64,
        chunk_tx: tokio::sync::mpsc::Sender<String>,
        reply: tokio::sync::oneshot::Sender<Result<String, DenoError>>,
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
    pub fn spawn(
        index: usize,
        max_heap_size_mb: usize,
        render_timeout_ms: u64,
        node_builtins: bool,
    ) -> Result<Self, DenoError> {
        let (tx, rx) = tokio::sync::mpsc::channel::<WorkerMsg>(1);
        let (init_tx, init_rx) = mpsc::sync_channel::<Result<(), String>>(1);

        std::thread::Builder::new()
            .name(format!("deno-worker-{index}"))
            .spawn(move || worker_thread_main(rx, init_tx, max_heap_size_mb, node_builtins))
            .map_err(|e| {
                DenoError::WorkerInit(format!("Failed to spawn isolate thread {index}: {e}"))
            })?;

        init_rx
            .recv()
            .map_err(|_| {
                DenoError::WorkerInit("Isolate thread exited unexpectedly during init".into())
            })?
            .map_err(DenoError::WorkerInit)?;

        Ok(Self {
            tx,
            render_timeout_ms,
        })
    }

    /// Sends a render request to this isolate's worker thread and blocks
    /// until the result arrives. Returns the result as a JSON string so any
    /// JS type survives the boundary.
    pub fn block_on_render(&self, bundle_id: &str, args_json: &str) -> Result<String, DenoError> {
        let (reply_tx, reply_rx) = std::sync::mpsc::sync_channel::<Result<String, DenoError>>(1);
        let hang_timeout = Duration::from_millis(self.render_timeout_ms + 100);

        self.tx
            .blocking_send(WorkerMsg::Render {
                bundle_id: bundle_id.to_string(),
                args_json: args_json.to_string(),
                render_timeout_ms: self.render_timeout_ms,
                reply: reply_tx,
            })
            .map_err(|_| DenoError::WorkerDied("Deno worker thread has exited".into()))?;

        match reply_rx.recv_timeout(hang_timeout) {
            Ok(result) => result,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(DenoError::Render(format!(
                "Render process hung after {}ms",
                hang_timeout.as_millis()
            ))),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(DenoError::WorkerDied(
                "Deno worker thread exited before sending a reply".into(),
            )),
        }
    }

    /// Sends a streaming render request and returns the final result.
    /// The worker thread runs the V8 event loop during the render.
    pub fn block_on_render_stream(
        &self,
        bundle_id: &str,
        args_json: &str,
    ) -> Result<String, DenoError> {
        let (reply_tx, reply_rx) =
            tokio::sync::oneshot::channel::<Result<String, DenoError>>();
        let (chunk_tx, _chunk_rx) = tokio::sync::mpsc::channel::<String>(64);

        self.tx
            .blocking_send(WorkerMsg::RenderStream {
                bundle_id: bundle_id.to_string(),
                args_json: args_json.to_string(),
                render_timeout_ms: self.render_timeout_ms,
                chunk_tx,
                reply: reply_tx,
            })
            .map_err(|_| DenoError::WorkerDied("Deno worker thread has exited".into()))?;

        reply_rx
            .blocking_recv()
            .map_err(|_| DenoError::WorkerDied("Deno worker thread exited before reply".into()))?
    }

    /// Queries V8 heap statistics from this isolate's thread.
    pub fn block_on_heap_stats(&self) -> Result<String, DenoError> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();

        self.tx
            .blocking_send(WorkerMsg::HeapStats { reply: reply_tx })
            .map_err(|_| DenoError::WorkerDied("Deno worker thread has exited".into()))?;

        reply_rx.blocking_recv().map_err(|_| {
            DenoError::WorkerDied("Deno worker thread exited before sending a reply".into())
        })?
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
    pub fn new(
        size: usize,
        max_heap_size_mb: usize,
        render_timeout_ms: u64,
        node_builtins: bool,
    ) -> Result<Self, DenoError> {
        validate_pool_size(size)?;

        let mut handles = Vec::with_capacity(size);
        for i in 0..size {
            let handle =
                IsolateHandle::spawn(i, max_heap_size_mb, render_timeout_ms, node_builtins)?;
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

    /// Dispatches a streaming render request to the next available isolate.
    pub fn dispatch_render_stream(
        &self,
        bundle_id: &str,
        args_json: &str,
    ) -> Result<String, DenoError> {
        self.next_handle().block_on_render_stream(bundle_id, args_json)
    }

    /// Queries V8 heap statistics from the next available isolate.
    pub fn heap_stats(&self) -> Result<String, DenoError> {
        self.next_handle().block_on_heap_stats()
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
    node_builtins: bool,
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

        let oom_triggered = Arc::new(AtomicBool::new(false));

        let mut worker = match build_worker(
            &main_module_url, max_heap_size_mb, node_builtins, oom_triggered.clone(),
        ) {
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
                    let result = load_bundle_in_worker(
                        &mut worker,
                        &bundle_id,
                        bundle_code,
                        script_name,
                        node_builtins,
                    );
                    let _ = reply.send(result);
                }
                WorkerMsg::Render {
                    bundle_id,
                    args_json,
                    render_timeout_ms,
                    reply,
                } => {
                    let result = call_render(
                        &mut worker, &bundle_id, &args_json, render_timeout_ms, &oom_triggered,
                    );
                    let _ = reply.send(result);
                }
                WorkerMsg::HeapStats { reply } => {
                    let result = collect_heap_stats(&mut worker);
                    let _ = reply.send(result);
                }
                WorkerMsg::RenderStream {
                    bundle_id,
                    args_json,
                    render_timeout_ms,
                    chunk_tx,
                    reply,
                } => {
                    let result = render_stream::render_streaming(
                        &mut worker, &bundle_id, &args_json,
                        render_timeout_ms, chunk_tx, &oom_triggered,
                    ).await;
                    let _ = reply.send(result);
                }
            }
        }
    });
}

/// Injects `globalThis.require` into the V8 context by loading
/// `createRequire` from Deno's built-in `node:module` via async import.
fn setup_require(worker: &mut MainWorker) -> Result<(), String> {
    // Idempotency guard: skip the async import + microtask polling when
    // `globalThis.require` is already set from a prior bundle load into
    // the same isolate. Saves ~10ms per subsequent bundle load.
    let check_val = worker
        .execute_script(
            "<ssr-deno:require-guard>",
            "typeof globalThis.require !== 'undefined'".to_string().into(),
        )
        .map_err(|e| format!("Failed to check require: {e}"))?;
    let isolate = worker.js_runtime.v8_isolate();
    let check_ref = check_val.open(isolate);
    if check_ref.is_true() {
        return Ok(());
    }

    // The deno_node extension registers node:module polyfill via its extension
    // system. When import('node:module') is called, the extension serves the
    // source code directly (not through the module loader). We use microtask
    // polling to let the async import resolve synchronously.
    worker
        .execute_script(
            "<ssr-deno:require>",
            r#"
            globalThis.__ssr_require_promise = (async () => {
                const { createRequire } = await import('node:module');
                globalThis.require = createRequire('file:///');
            })();
            "#
            .to_string()
            .into(),
        )
        .map_err(|e| format!("Failed to start require import: {e}"))?;

    let isolate = worker.js_runtime.v8_isolate();
    let deadline = Instant::now() + Duration::from_millis(10);
    while Instant::now() < deadline {
        isolate.perform_microtask_checkpoint();
        std::thread::sleep(Duration::from_micros(100));
    }

    worker
        .execute_script(
            "<ssr-deno:require-verify>",
            r#"
            if (typeof globalThis.require === 'undefined') {
                throw new Error('createRequire failed - globalThis.require is undefined');
            }
            "#.to_string().into(),
        )
        .map(|_| ())
        .map_err(|e| format!("setup_require failed: {e}"))
}

/// Evaluates the bundle code and moves `globalThis.render` into the bundle
/// namespace: `globalThis.__ssr_bundles[bundle_id] = { render: globalThis.render }`.
fn load_bundle_in_worker(
    worker: &mut MainWorker,
    bundle_id: &str,
    bundle_code: String,
    script_name: &'static str,
    node_builtins: bool,
) -> Result<(), String> {
    // Provide globalThis.require for bundles that use Node.js built-in modules.
    // Only needed when node_builtins is enabled.
    if node_builtins {
        if let Err(e) = setup_require(worker) {
            return Err(format!("Failed to set up require: {e}"));
        }
    }

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

fn build_worker(
    main_module: &Url,
    max_heap_size_mb: usize,
    node_builtins: bool,
    oom_triggered: Arc<AtomicBool>,
) -> Result<MainWorker, String> {
    let node_services = if node_builtins {
        use std::borrow::Cow;
        use node_resolver::{
            DenoIsBuiltInNodeModuleChecker,
            NodeResolverOptions, NodeConditionOptions,
            PackageJsonResolver,
            cache::NodeResolutionSys,
        };
        use deno_runtime::deno_fs::sync::MaybeArc;
        use deno_runtime::deno_node::{NodeResolver, NodeExtInitServices, NodeRequireLoaderRc};

        let loader: NodeRequireLoaderRc = std::rc::Rc::new(DenoNodeRequireLoader);

        let pkg_json_resolver = MaybeArc::new(
            PackageJsonResolver::new(Sys, None),
        );

        let resolver: MaybeArc<
            NodeResolver<NopInNpmPackageChecker, NopNpmPackageFolderResolver, Sys>,
        > = {
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
    } else {
        None
    };

    let module_loader: std::rc::Rc<dyn deno_runtime::deno_core::ModuleLoader> = if node_builtins {
        std::rc::Rc::new(NodeBuiltinOnlyModuleLoader)
    } else {
        std::rc::Rc::new(deno_runtime::deno_core::NoopModuleLoader)
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
        extensions: vec![
            deno_runtime::deno_core::Extension {
                name: "ssr_stream",
                ops: Cow::Owned(vec![render_stream::op_ssr_push_chunk()]),
                ..Default::default()
            },
        ],
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
        NopInNpmPackageChecker,
        NopNpmPackageFolderResolver,
        Sys,
    >(main_module, services, options);

    // Register a near-heap-limit callback on the V8 isolate to prevent
    // fatal OOM aborts. When V8 detects the heap is near its configured
    // limit (max_heap_size_mb), this callback sets the oom_triggered flag,
    // doubles the limit (buying one more GC cycle), and terminates the
    // running JS execution so that the OOM is caught as a RenderError
    // instead of a SIGTRAP.
    //
    // Without this, a user component that leaks memory across renders
    // eventually causes V8 to call abort(), killing the Ruby process.
    let isolate_handle = worker.js_runtime.v8_isolate().thread_safe_handle();
    worker.js_runtime.add_near_heap_limit_callback(
        move |current_limit, _initial_limit| {
            oom_triggered.store(true, Ordering::SeqCst);
            let _ = isolate_handle.terminate_execution();
            current_limit * 2
        },
    );

    Ok(worker)
}
