mod deno_runtime_wrapper;
mod node_builtin_loader;
mod nop_types;
mod require_loader;
mod sys;

use std::path::Path;
use std::sync::{Mutex, MutexGuard, OnceLock, RwLock};

use deno_runtime_wrapper::{IsolatePool, SSRDenoError};
use magnus::value::ReprValue;
use magnus::{block::Yield, function, method, Error, ExceptionClass, Module, Object, Ruby, Value};
use ssr_deno_core::source_mapper::SsrSourceMapper;
use ssr_deno_core::{max_heap_size_mb_checked, validate_render_timeout_ms, Config};

fn get_source_mapper() -> &'static RwLock<SsrSourceMapper> {
    static MAPPER: OnceLock<RwLock<SsrSourceMapper>> = OnceLock::new();
    MAPPER.get_or_init(|| RwLock::new(SsrSourceMapper::new()))
}

// Recover from poisoned mutex instead of panicking. Poison happens if a thread
// panics while holding the lock — extremely rare, but unrecoverable if we
// propagate via `.unwrap()`.
fn lock_config() -> MutexGuard<'static, Config> {
    CONFIG.lock().unwrap_or_else(|e| e.into_inner())
}

// ---------------------------------------------------------------------------
// GVL release — rb_thread_call_without_gvl from Ruby's C API
// ---------------------------------------------------------------------------

// SAFETY: `rb_thread_call_without_gvl` is part of Ruby's C API (`<ruby/thread.h>`).
// The callback (`func`) must not touch Ruby objects, use Ruby APIs, or panic
// through the FFI boundary. `ubf` (unblock function) must be signal-safe if set.
// We pass `None` for `ubf`, so no unblock-function constraint applies.
extern "C" {
    fn rb_thread_call_without_gvl(
        func: unsafe extern "C" fn(*mut std::ffi::c_void) -> *mut std::ffi::c_void,
        data1: *mut std::ffi::c_void,
        ubf: Option<unsafe extern "C" fn(*mut std::ffi::c_void)>,
        data2: *mut std::ffi::c_void,
    ) -> *mut std::ffi::c_void;
}

struct RenderArgs {
    pool: &'static IsolatePool,
    bundle_id: String,
    args_json: String,
}

struct RawRenderResult {
    result: Result<String, SSRDenoError>,
}

// SAFETY: `data` is a `Box<RenderArgs>` leaked by `Box::into_raw` in `native_render`.
// Ownership is reclaimed here via `Box::from_raw`. The `RenderArgs` contain a
// `&'static IsolatePool` and owned `String`s — no Ruby objects, so the callback
// is safe to run without the GVL (as required by `rb_thread_call_without_gvl`).
// The returned `Box<RawRenderResult>` is re-boxed as a raw pointer for the caller.
unsafe extern "C" fn render_worker(data: *mut std::ffi::c_void) -> *mut std::ffi::c_void {
    let args = Box::from_raw(data as *mut RenderArgs);
    let result = args.pool.dispatch_render(&args.bundle_id, &args.args_json);
    Box::into_raw(Box::new(RawRenderResult { result })) as *mut std::ffi::c_void
}

static POOL: OnceLock<IsolatePool> = OnceLock::new();
static POOL_INIT_LOCK: Mutex<()> = Mutex::new(());

// Defaults: 64 MB heap, 1 isolate pool.
static CONFIG: Mutex<Config> = Mutex::new(Config::default());

/// Returns an error if the runtime has already been initialized.
/// All config setters call this before modifying CONFIG.
fn check_not_initialized(ruby: &Ruby) -> Result<(), Error> {
    if POOL.get().is_some() {
        Err(Error::new(
            deno_exception_class(ruby, "JsRuntimeInitializationError"),
            "Cannot set config after runtime is already initialized",
        ))
    } else {
        Ok(())
    }
}

// Looks up an exception class by name inside the SSR::Deno Ruby module.
fn deno_exception_class(ruby: &Ruby, name: &'static str) -> ExceptionClass {
    ruby.define_module("SSR")
        .and_then(|m| m.define_module("Deno"))
        .and_then(|m| m.const_get(name))
        .unwrap_or_else(|_| ruby.exception_runtime_error())
}

fn js_runtime_initialization_error(ruby: &Ruby, msg: impl Into<String>) -> Error {
    Error::new(
        deno_exception_class(ruby, "JsRuntimeInitializationError"),
        msg.into(),
    )
}

fn js_runtime_not_initialized_error(ruby: &Ruby, msg: impl Into<String>) -> Error {
    Error::new(
        deno_exception_class(ruby, "JsRuntimeNotInitializedError"),
        msg.into(),
    )
}

fn js_runtime_worker_error(ruby: &Ruby, msg: impl Into<String>) -> Error {
    Error::new(
        deno_exception_class(ruby, "JsRuntimeWorkerError"),
        msg.into(),
    )
}

fn bundle_not_found_error(ruby: &Ruby, msg: impl Into<String>) -> Error {
    Error::new(
        deno_exception_class(ruby, "BundleNotFoundError"),
        msg.into(),
    )
}

fn render_error(ruby: &Ruby, msg: impl Into<String>) -> Error {
    Error::new(deno_exception_class(ruby, "RenderError"), msg.into())
}

fn js_runtime_out_of_memory_error(ruby: &Ruby, msg: impl Into<String>) -> Error {
    Error::new(
        deno_exception_class(ruby, "JsRuntimeOutOfMemoryError"),
        msg.into(),
    )
}

fn heap_stats_serialization_error(ruby: &Ruby, msg: impl Into<String>) -> Error {
    Error::new(
        deno_exception_class(ruby, "HeapStatsSerializationError"),
        msg.into(),
    )
}

fn map_render_error(ruby: &Ruby, e: SSRDenoError) -> Error {
    match e {
        SSRDenoError::Render(msg) => {
            let resolved = get_source_mapper()
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .resolve(&msg);
            render_error(ruby, resolved)
        }
        SSRDenoError::WorkerDied(msg) => js_runtime_worker_error(ruby, msg),
        SSRDenoError::BundleNotFound(msg) => bundle_not_found_error(ruby, msg),
        SSRDenoError::OutOfMemory(msg) => js_runtime_out_of_memory_error(ruby, msg),
        SSRDenoError::BundleLoad(msg) => js_runtime_initialization_error(ruby, msg),
        SSRDenoError::WorkerInit(msg) => js_runtime_initialization_error(ruby, msg),
        SSRDenoError::HeapStatsSerialization(msg) => heap_stats_serialization_error(ruby, msg),
    }
}

/// Called by Ruby before the first Bundle.new to configure the V8 heap limit.
/// Must be called before any native_load_bundle or native_render call.
///
/// Validates that the value doesn't overflow when converted to bytes.
/// The max safe value is usize::MAX / 1024 / 1024 (~16 TB on 64-bit),
/// which is far beyond any practical V8 heap limit.
fn native_set_node_builtins_enabled(ruby: &Ruby, enabled: bool) -> Result<(), Error> {
    check_not_initialized(ruby)?;
    lock_config().node_builtins = enabled;
    Ok(())
}

fn native_set_max_heap_size_mb(ruby: &Ruby, mb: usize) -> Result<(), Error> {
    if let Err(msg) = max_heap_size_mb_checked(mb) {
        return Err(Error::new(
            ruby.exception_arg_error(),
            format!("{msg} (max: {})", usize::MAX / 1024 / 1024),
        ));
    }

    check_not_initialized(ruby)?;
    lock_config().max_heap_size_mb = mb;
    Ok(())
}

/// Called by Ruby before the first Bundle.new to configure the isolate pool size.
fn native_set_isolate_pool_size(ruby: &Ruby, size: usize) -> Result<(), Error> {
    check_not_initialized(ruby)?;
    lock_config().isolate_pool_size = size;
    Ok(())
}

/// Called by Ruby before the first Bundle.new to configure the render timeout.
///
/// Validates that `ms` is within [100, 300000].
fn native_set_render_timeout_ms(ruby: &Ruby, ms: u64) -> Result<(), Error> {
    if let Err(msg) = validate_render_timeout_ms(ms) {
        return Err(Error::new(ruby.exception_arg_error(), msg));
    }
    check_not_initialized(ruby)?;
    lock_config().render_timeout_ms = ms;
    Ok(())
}

// ---------------------------------------------------------------------------
// Native getter functions — read CONFIG without initialization check
// ---------------------------------------------------------------------------

fn native_get_max_heap_size_mb() -> usize {
    lock_config().max_heap_size_mb
}

fn native_get_isolate_pool_size() -> usize {
    lock_config().isolate_pool_size
}

fn native_get_render_timeout_ms() -> u64 {
    lock_config().render_timeout_ms
}

fn native_get_node_builtins_enabled() -> bool {
    lock_config().node_builtins
}

fn native_set_source_maps_enabled(ruby: &Ruby, enabled: bool) -> Result<(), Error> {
    check_not_initialized(ruby)?;
    lock_config().source_maps = enabled;
    Ok(())
}

fn native_get_source_maps_enabled() -> bool {
    lock_config().source_maps
}

// ---------------------------------------------------------------------------
// Pool initialization (OnceLock + init mutex)
// ---------------------------------------------------------------------------

fn get_or_init_pool(ruby: &Ruby) -> Result<&'static IsolatePool, Error> {
    if let Some(p) = POOL.get() {
        return Ok(p);
    }
    let _guard = POOL_INIT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(p) = POOL.get() {
        return Ok(p);
    }

    let config = *lock_config();
    let pool_size = ssr_deno_core::resolve_pool_size(config);
    let max_heap_size_mb = config.max_heap_size_mb;
    let render_timeout_ms = config.render_timeout_ms;
    let node_builtins = config.node_builtins;

    let pool = IsolatePool::new(
        pool_size,
        max_heap_size_mb,
        render_timeout_ms,
        node_builtins,
    )
    .map_err(|e| js_runtime_initialization_error(ruby, e.to_string()))?;
    let _ = POOL.set(pool);
    Ok(POOL.get().expect("pool was just initialized"))
}

fn get_pool(ruby: &Ruby) -> Result<&'static IsolatePool, Error> {
    POOL.get().ok_or_else(|| {
        js_runtime_not_initialized_error(
            ruby,
            "Runtime not initialized. Call `SSR::Deno::Bundle.new` first.",
        )
    })
}

// ---------------------------------------------------------------------------
// Native methods callable from Ruby
// ---------------------------------------------------------------------------

/// Loads a bundle into every isolate in the pool, registering its render
/// function under `globalThis.__ssr_bundles[bundle_id]`.
/// Initializes the pool lazily on first call.
/// If source maps are enabled, reads `.js.map` sidecar and registers it.
fn native_load_bundle(ruby: &Ruby, bundle_id: String, bundle_path: String) -> Result<(), Error> {
    get_or_init_pool(ruby)?
        .load_bundle(&bundle_id, &bundle_path)
        .map_err(|e| js_runtime_initialization_error(ruby, e.to_string()))?;

    if lock_config().source_maps {
        let map_path = Path::new(&bundle_path).with_extension("js.map");
        let script_name = Path::new(&bundle_path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("(unknown)");
        get_source_mapper()
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .register(script_name, &map_path);
    }

    Ok(())
}

/// Dispatches a render request to the next available isolate.
/// Runs the full Deno event loop (macrotasks, timers fire).
/// Releases GVL during blocking channel recv so other Ruby threads
/// can enter the FFI boundary concurrently.
/// Returns the result as a JSON string so any JS type survives the boundary.
fn native_render(ruby: &Ruby, bundle_id: String, args_json: String) -> Result<String, Error> {
    let pool = get_pool(ruby)?;

    let args = Box::new(RenderArgs {
        pool,
        bundle_id,
        args_json,
    });

    let result_ptr = unsafe {
        let ptr = Box::into_raw(args) as *mut std::ffi::c_void;
        rb_thread_call_without_gvl(render_worker, ptr, None, std::ptr::null_mut())
    };

    let raw = unsafe { Box::from_raw(result_ptr as *mut RawRenderResult) };
    raw.result.map_err(|e| map_render_error(ruby, e))
}

/// Returns the version of the ssr_deno native extension.
fn native_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Queries V8 heap statistics from the isolate pool.
fn native_heap_stats(ruby: &Ruby) -> Result<String, Error> {
    get_pool(ruby)?
        .heap_stats()
        .map_err(|e| map_render_error(ruby, e))
}

/// Dispatches a chunked render. Yields each HTML chunk to the provided block
/// as it arrives from the JS render function.
///
/// If no block is given, returns an Enumerator (Ruby's standard pattern).
/// When a block IS given, yields chunks incrementally and raises
/// `SSR::Deno::RenderError` if the render fails during chunk delivery.
///
/// The Ruby thread blocks on each `chunk_rx.blocking_recv()` between yields.
fn native_render_chunks(
    ruby: &Ruby,
    rb_self: Value,
    bundle_id: String,
    args_json: String,
) -> Result<Yield<impl Iterator<Item = String>>, Error> {
    if !ruby.block_given() {
        return Ok(Yield::Enumerator(
            rb_self.enumeratorize("native_render_chunks", (bundle_id, args_json)),
        ));
    }

    let (mut chunk_rx, reply_rx) = get_pool(ruby)?
        .dispatch_render_chunked(&bundle_id, &args_json)
        .map_err(|e| map_render_error(ruby, e))?;

    // Yield chunks to the block until the channel closes.
    while let Some(chunk) = chunk_rx.blocking_recv() {
        // yield_value uses protect internally — safe against block break.
        let _: Value = ruby.yield_value(ruby.str_new(&chunk))?;
    }

    // Channel closed — check if the render completed successfully or errored.
    match reply_rx.blocking_recv() {
        Ok(Ok(())) => {
            // Success — return empty iter (block was already given all chunks).
            Ok(Yield::Iter(std::iter::empty()))
        }
        Ok(Err(e)) => Err(map_render_error(ruby, e)),
        Err(_) => Err(map_render_error(
            ruby,
            SSRDenoError::WorkerDied(
                "Deno worker thread exited before signaling render completion".into(),
            ),
        )),
    }
}

// ---------------------------------------------------------------------------
// Dev-mode FFI stubs — fix dispatch surface before any logic
// ---------------------------------------------------------------------------

/// Opaque Ruby-side handle to a dev worker. Cannot be forged from Ruby
/// (constructible only by Rust via `native_dev_worker_new`). Holds an
/// `Arc<DevIsolateHandle>` so multiple Ruby refs to the same worker stay
/// alive until the last Ruby ref is GCed.
#[cfg(feature = "dev-mode")]
#[magnus::wrap(class = "SSR::Deno::DevWorkerHandle", free_immediately, size)]
pub struct DevWorkerHandle(
    pub std::sync::Arc<deno_runtime_wrapper::dev_handle::DevIsolateHandle>,
);

#[cfg(feature = "dev-mode")]
fn native_dev_worker_new(
    ruby: &Ruby,
    project_root: String,
    max_heap_size_mb: usize,
    render_timeout_ms: u64,
) -> Result<DevWorkerHandle, Error> {
    let _ = (project_root, max_heap_size_mb, render_timeout_ms);
    Err(js_runtime_initialization_error(
        ruby,
        "dev-mode not yet implemented",
    ))
}

#[cfg(feature = "dev-mode")]
fn native_dev_load_entry(
    ruby: &Ruby,
    _handle: &DevWorkerHandle,
    _entry_path: String,
    _alias_map_json: String,
) -> Result<(), Error> {
    Err(js_runtime_initialization_error(
        ruby,
        "dev-mode not yet implemented",
    ))
}

#[cfg(feature = "dev-mode")]
fn native_dev_render(
    ruby: &Ruby,
    _handle: &DevWorkerHandle,
    _bundle_id: String,
    _args_json: String,
) -> Result<String, Error> {
    Err(render_error(ruby, "dev-mode not yet implemented"))
}

#[cfg(feature = "dev-mode")]
fn native_dev_render_chunks(
    ruby: &Ruby,
    _rb_self: Value,
    _handle: &DevWorkerHandle,
    _bundle_id: String,
    _args_json: String,
) -> Result<Yield<impl Iterator<Item = String>>, Error> {
    Err::<Yield<std::iter::Empty<String>>, Error>(render_error(
        ruby,
        "dev-mode not yet implemented",
    ))
}

/// The magnus init function — called when Ruby loads the native extension.
/// Registers the `SSR::Deno` module, its exception hierarchy, and its methods.
#[magnus::init]
fn init(ruby: &Ruby) -> Result<(), Error> {
    // Opt in to Ractor safety. All shared state (POOL) is Rust-level and
    // protected by OnceLock. Renders dispatch through per-isolate tokio
    // channels and the round-robin counter uses AtomicUsize, so concurrent
    // Ractors get isolated results without shared mutable state.
    unsafe {
        extern "C" {
            fn rb_ext_ractor_safe(flag: bool);
        }
        rb_ext_ractor_safe(true);
    }

    let module = ruby.define_module("SSR")?;
    let deno_module = module.define_module("Deno")?;

    let base_error = deno_module.define_error("Error", ruby.exception_standard_error())?;
    deno_module.define_error("JsRuntimeInitializationError", base_error)?;
    deno_module.define_error("JsRuntimeNotInitializedError", base_error)?;
    deno_module.define_error("JsRuntimeWorkerError", base_error)?;
    deno_module.define_error("BundleNotFoundError", base_error)?;
    deno_module.define_error("RenderError", base_error)?;
    deno_module.define_error("JsRuntimeOutOfMemoryError", base_error)?;
    deno_module.define_error("HeapStatsSerializationError", base_error)?;

    deno_module.define_singleton_method("native_load_bundle", function!(native_load_bundle, 2))?;
    deno_module.define_singleton_method("native_render", function!(native_render, 2))?;
    deno_module.define_singleton_method("native_version", function!(native_version, 0))?;
    deno_module.define_singleton_method(
        "native_set_max_heap_size_mb",
        function!(native_set_max_heap_size_mb, 1),
    )?;
    deno_module.define_singleton_method(
        "native_set_isolate_pool_size",
        function!(native_set_isolate_pool_size, 1),
    )?;
    deno_module.define_singleton_method(
        "native_set_render_timeout_ms",
        function!(native_set_render_timeout_ms, 1),
    )?;
    deno_module.define_singleton_method("native_heap_stats", function!(native_heap_stats, 0))?;
    deno_module.define_singleton_method(
        "native_set_node_builtins_enabled",
        function!(native_set_node_builtins_enabled, 1),
    )?;
    deno_module.define_singleton_method(
        "native_get_max_heap_size_mb",
        function!(native_get_max_heap_size_mb, 0),
    )?;
    deno_module.define_singleton_method(
        "native_get_isolate_pool_size",
        function!(native_get_isolate_pool_size, 0),
    )?;
    deno_module.define_singleton_method(
        "native_get_render_timeout_ms",
        function!(native_get_render_timeout_ms, 0),
    )?;
    deno_module.define_singleton_method(
        "native_get_node_builtins_enabled",
        function!(native_get_node_builtins_enabled, 0),
    )?;
    deno_module.define_singleton_method(
        "native_set_source_maps_enabled",
        function!(native_set_source_maps_enabled, 1),
    )?;
    deno_module.define_singleton_method(
        "native_get_source_maps_enabled",
        function!(native_get_source_maps_enabled, 0),
    )?;
    deno_module
        .define_singleton_method("native_render_chunks", method!(native_render_chunks, 2))?;

    #[cfg(feature = "dev-mode")]
    {
        // Register the opaque handle class. Ruby cannot construct it directly;
        // only Rust can return an instance via `native_dev_worker_new`.
        deno_module.define_class("DevWorkerHandle", ruby.class_object())?;

        deno_module.define_singleton_method(
            "native_dev_worker_new",
            function!(native_dev_worker_new, 3),
        )?;
        deno_module.define_singleton_method(
            "native_dev_load_entry",
            function!(native_dev_load_entry, 3),
        )?;
        deno_module
            .define_singleton_method("native_dev_render", function!(native_dev_render, 3))?;
        deno_module.define_singleton_method(
            "native_dev_render_chunks",
            method!(native_dev_render_chunks, 3),
        )?;
    }
    Ok(())
}
