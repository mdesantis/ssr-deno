mod deno_runtime_wrapper;
mod node_builtin_loader;
mod nop_types;
mod require_loader;
mod sys;

use deno_runtime_wrapper::{SSRDenoError, IsolatePool};
use magnus::{block::Yield, function, method, Error, ExceptionClass, Module, Object, Ruby, Value};
use magnus::value::ReprValue;
use ssr_deno_core::{max_heap_size_mb_checked, validate_render_timeout_ms, Config};
use std::sync::{Mutex, OnceLock};

static POOL: OnceLock<IsolatePool> = OnceLock::new();
static POOL_INIT_LOCK: Mutex<()> = Mutex::new(());
static INITIALIZED: OnceLock<()> = OnceLock::new();

// Defaults: 64 MB heap, 0 = auto-detect pool size from CPU count.
static CONFIG: Mutex<Config> = Mutex::new(Config::default());

/// Returns an error if the runtime has already been initialized.
/// All config setters call this before modifying CONFIG.
fn check_not_initialized() -> Result<(), Error> {
    if INITIALIZED.get().is_some() {
        Err(Error::new(
            deno_exception_class("JsRuntimeInitializationError"),
            "Cannot set config after runtime is already initialized",
        ))
    } else {
        Ok(())
    }
}

// Looks up an exception class by name inside the SSR::Deno Ruby module.
fn deno_exception_class(name: &'static str) -> ExceptionClass {
    let ruby = Ruby::get().unwrap();
    ruby.define_module("SSR")
        .and_then(|m| m.define_module("Deno"))
        .and_then(|m| m.const_get(name))
        .unwrap_or_else(|_| ruby.exception_runtime_error())
}

fn js_runtime_initialization_error(msg: impl Into<String>) -> Error {
    Error::new(
        deno_exception_class("JsRuntimeInitializationError"),
        msg.into(),
    )
}

fn js_runtime_not_initialized_error(msg: impl Into<String>) -> Error {
    Error::new(
        deno_exception_class("JsRuntimeNotInitializedError"),
        msg.into(),
    )
}

fn js_runtime_worker_error(msg: impl Into<String>) -> Error {
    Error::new(deno_exception_class("JsRuntimeWorkerError"), msg.into())
}

fn bundle_not_found_error(msg: impl Into<String>) -> Error {
    Error::new(deno_exception_class("BundleNotFoundError"), msg.into())
}

fn render_error(msg: impl Into<String>) -> Error {
    Error::new(deno_exception_class("RenderError"), msg.into())
}

fn js_runtime_out_of_memory_error(msg: impl Into<String>) -> Error {
    Error::new(deno_exception_class("JsRuntimeOutOfMemoryError"), msg.into())
}

fn heap_stats_serialization_error(msg: impl Into<String>) -> Error {
    Error::new(deno_exception_class("HeapStatsSerializationError"), msg.into())
}

fn map_render_error(e: SSRDenoError) -> Error {
    match e {
        SSRDenoError::WorkerDied(msg) => js_runtime_worker_error(msg),
        SSRDenoError::BundleNotFound(msg) => bundle_not_found_error(msg),
        SSRDenoError::Render(msg) => render_error(msg),
        SSRDenoError::OutOfMemory(msg) => js_runtime_out_of_memory_error(msg),
        SSRDenoError::BundleLoad(msg) => js_runtime_initialization_error(msg),
        SSRDenoError::WorkerInit(msg) => js_runtime_initialization_error(msg),
        SSRDenoError::HeapStatsSerialization(msg) => heap_stats_serialization_error(msg),
    }
}

/// Called by Ruby before the first Bundle.new to configure the V8 heap limit.
/// Must be called before any native_load_bundle or native_render call.
///
/// Validates that the value doesn't overflow when converted to bytes.
/// The max safe value is usize::MAX / 1024 / 1024 (~16 TB on 64-bit),
/// which is far beyond any practical V8 heap limit.
fn native_set_node_builtins_enabled(enabled: bool) -> Result<(), Error> {
    check_not_initialized()?;
    CONFIG.lock().unwrap().node_builtins = enabled;
    Ok(())
}

fn native_set_max_heap_size_mb(mb: usize) -> Result<(), Error> {
    // Validate overflow before touching any state.
    if let Err(msg) = max_heap_size_mb_checked(mb) {
        return Err(Error::new(
            Ruby::get().unwrap().exception_arg_error(),
            format!("{msg} (max: {})", usize::MAX / 1024 / 1024),
        ));
    }

    check_not_initialized()?;
    CONFIG.lock().unwrap().max_heap_size_mb = mb;
    Ok(())
}

/// Called by Ruby before the first Bundle.new to configure the isolate pool size.
/// A value of 0 means auto-detect from CPU count. Must be called before any
/// native_load_bundle or native_render call.
fn native_set_isolate_pool_size(size: usize) -> Result<(), Error> {
    check_not_initialized()?;
    CONFIG.lock().unwrap().isolate_pool_size = size;
    Ok(())
}

/// Called by Ruby before the first Bundle.new to configure the render timeout.
/// Must be called before any native_load_bundle or native_render call.
///
/// Validates that `ms` is within [100, 300000].
fn native_set_render_timeout_ms(ms: u64) -> Result<(), Error> {
    if let Err(msg) = validate_render_timeout_ms(ms) {
        return Err(Error::new(
            Ruby::get().unwrap().exception_arg_error(),
            msg,
        ));
    }
    check_not_initialized()?;
    CONFIG.lock().unwrap().render_timeout_ms = ms;
    Ok(())
}

// ---------------------------------------------------------------------------
// Native getter functions — read CONFIG without initialization check
// ---------------------------------------------------------------------------

fn native_get_max_heap_size_mb() -> usize {
    CONFIG.lock().unwrap().max_heap_size_mb
}

fn native_get_isolate_pool_size() -> usize {
    CONFIG.lock().unwrap().isolate_pool_size
}

fn native_get_render_timeout_ms() -> u64 {
    CONFIG.lock().unwrap().render_timeout_ms
}

fn native_get_node_builtins_enabled() -> bool {
    CONFIG.lock().unwrap().node_builtins
}

// ---------------------------------------------------------------------------
// Pool initialization (OnceLock + init mutex)
//   OnceLock provides lock-free reads after init.
//   POOL_INIT_LOCK prevents duplicate pool creation during the init window.
// ---------------------------------------------------------------------------

// TODO: replace with OnceLock::get_or_try_init once stabilised (tracking issue #109737).
fn get_or_init_pool() -> Result<&'static IsolatePool, Error> {
    if let Some(p) = POOL.get() {
        return Ok(p);
    }
    let _guard = POOL_INIT_LOCK.lock().unwrap();
    if let Some(p) = POOL.get() {
        return Ok(p);
    }

    let config = *CONFIG.lock().unwrap();
    let pool_size = ssr_deno_core::resolve_pool_size(config);
    // max_heap_size_mb is a per-isolate V8 CreateParams constraint, NOT a
    // total budget. Each isolate independently gets the configured limit so
    // that workloads calibrated for the single-isolate case don't break when
    // the pool auto-detects more cores. Users with tight memory can reduce
    // the per-isolate limit explicitly.
    let max_heap_size_mb = config.max_heap_size_mb;
    let render_timeout_ms = config.render_timeout_ms;
    let node_builtins = config.node_builtins;

    let pool =
        IsolatePool::new(pool_size, max_heap_size_mb, render_timeout_ms, node_builtins)
            .map_err(|e| js_runtime_initialization_error(e.to_string()))?;
    let _ = POOL.set(pool);
    let _ = INITIALIZED.set(());
    Ok(POOL.get().unwrap())
}

fn get_pool() -> Result<&'static IsolatePool, Error> {
    POOL.get().ok_or_else(|| {
        js_runtime_not_initialized_error(
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
fn native_load_bundle(bundle_id: String, bundle_path: String) -> Result<(), Error> {
    get_or_init_pool()?
        .load_bundle(&bundle_id, &bundle_path)
        .map_err(|e| js_runtime_initialization_error(e.to_string()))
}

/// Dispatches a render request to the next available isolate.
/// Runs the full Deno event loop (macrotasks, timers fire).
/// Returns the result as a JSON string so any JS type survives the boundary.
fn native_render(bundle_id: String, args_json: String) -> Result<String, Error> {
    get_pool()?
        .dispatch_render(&bundle_id, &args_json)
        .map_err(map_render_error)
}

/// Returns the version of the ssr_deno native extension.
fn native_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Queries V8 heap statistics from the isolate pool.
fn native_heap_stats() -> Result<String, Error> {
    get_pool()?
        .heap_stats()
        .map_err(map_render_error)
}

/// Dispatches a chunked render. Yields each HTML chunk to the provided block
/// as it arrives from React's `renderToPipeableStream`.
///
/// If no block is given, returns an Enumerator (Ruby's standard pattern).
/// When a block IS given, yields chunks incrementally and raises
/// `SSR::Deno::RenderError` if the render fails mid-stream.
///
/// The Ruby thread blocks on each `chunk_rx.blocking_recv()` between yields.
fn native_render_stream_chunks(
    ruby: &Ruby,
    rb_self: Value,
    bundle_id: String,
    args_json: String,
) -> Result<Yield<impl Iterator<Item = String>>, Error> {
    if !ruby.block_given() {
        return Ok(Yield::Enumerator(
            rb_self.enumeratorize("native_render_stream_chunks", (bundle_id, args_json)),
        ));
    }

    let (mut chunk_rx, reply_rx) = get_pool()?
        .dispatch_render_chunked(&bundle_id, &args_json)
        .map_err(map_render_error)?;

    // Yield chunks to the block until the channel closes.
    loop {
        match chunk_rx.blocking_recv() {
            Some(chunk) => {
                // yield_value uses protect internally — safe against block break.
                let _: Value = ruby.yield_value(ruby.str_new(&chunk))?;
            }
            None => break,
        }
    }

    // Channel closed — check if the render completed successfully or errored.
    match reply_rx.blocking_recv() {
        Ok(Ok(())) => {
            // Success — return empty iter (block was already given all chunks).
            Ok(Yield::Iter(std::iter::empty()))
        }
        Ok(Err(e)) => Err(map_render_error(e)),
        Err(_) => Err(map_render_error(SSRDenoError::WorkerDied(
            "Deno worker thread exited before signaling stream completion".into(),
        ))),
    }
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
    deno_module.define_singleton_method(
        "native_heap_stats",
        function!(native_heap_stats, 0),
    )?;
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
        "native_render_stream_chunks",
        method!(native_render_stream_chunks, 2),
    )?;
    Ok(())
}
