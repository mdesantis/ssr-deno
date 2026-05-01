mod deno_runtime_wrapper;
mod nop_types;
mod sys;

use deno_runtime_wrapper::{DenoError, IsolatePool};
use magnus::{function, Error, ExceptionClass, Module, Object, Ruby};
use std::sync::{Mutex, OnceLock};

// Hard cap: isolates beyond this use diminishing returns and eat memory.
const MAX_ISOLATES: usize = 8;

static POOL: OnceLock<IsolatePool> = OnceLock::new();
static INIT_LOCK: Mutex<()> = Mutex::new(());
static INITIALIZED: OnceLock<()> = OnceLock::new();

/// Configuration passed from Ruby to Rust before runtime initialization.
/// Stored in a Mutex so multiple setters can update individual fields before
/// the runtime is initialized.
/// Defaults are safe for unconfigured usage.
#[derive(Clone, Copy)]
struct Config {
    max_heap_size_mb: usize,
    isolate_pool_size: usize, // 0 = auto-detect from CPU count
}

// Defaults: 64 MB heap, 0 = auto-detect pool size from CPU count.
static CONFIG: Mutex<Config> = Mutex::new(Config {
    max_heap_size_mb: 64,
    isolate_pool_size: 0,
});

/// Returns an error if the runtime has already been initialized.
/// All config setters call this before modifying CONFIG.
fn check_not_initialized() -> Result<(), Error> {
    if INITIALIZED.get().is_some() {
        Err(Error::new(
            deno_exc("JsRuntimeInitializationError"),
            "Cannot set config after runtime is already initialized",
        ))
    } else {
        Ok(())
    }
}

// Looks up an exception class by name inside the SSR::Deno Ruby module.
fn deno_exc(name: &'static str) -> ExceptionClass {
    let ruby = Ruby::get().unwrap();
    ruby.define_module("SSR")
        .and_then(|m| m.define_module("Deno"))
        .and_then(|m| m.const_get(name))
        .unwrap_or_else(|_| ruby.exception_runtime_error())
}

fn js_runtime_initialization_error(msg: impl Into<String>) -> Error {
    Error::new(deno_exc("JsRuntimeInitializationError"), msg.into())
}

fn js_runtime_not_initialized_error(msg: impl Into<String>) -> Error {
    Error::new(deno_exc("JsRuntimeNotInitializedError"), msg.into())
}

fn js_runtime_worker_error(msg: impl Into<String>) -> Error {
    Error::new(deno_exc("JsRuntimeWorkerError"), msg.into())
}

fn bundle_not_found_error(msg: impl Into<String>) -> Error {
    Error::new(deno_exc("BundleNotFoundError"), msg.into())
}

fn render_error(msg: impl Into<String>) -> Error {
    Error::new(deno_exc("RenderError"), msg.into())
}

fn map_render_error(e: DenoError) -> Error {
    match e {
        DenoError::WorkerDied(msg) => js_runtime_worker_error(msg),
        DenoError::BundleNotFound(msg) => bundle_not_found_error(msg),
        DenoError::Render(msg) => render_error(msg),
        other => js_runtime_worker_error(other.to_string()),
    }
}

/// Called by Ruby before the first Bundle.new to configure the V8 heap limit.
/// Must be called before any native_load_bundle or native_render call.
///
/// Validates that the value doesn't overflow when converted to bytes.
/// The max safe value is usize::MAX / 1024 / 1024 (~16 TB on 64-bit),
/// which is far beyond any practical V8 heap limit.
fn native_set_max_heap_size_mb(mb: usize) -> Result<(), Error> {
    // Check that mb * 1024 * 1024 doesn't overflow usize.
    // On 64-bit: max ≈ 16,384,000 MB (16 TB). On 32-bit: max ≈ 4,096 MB.
    mb.checked_mul(1024 * 1024)
        .ok_or_else(|| {
            Error::new(
                Ruby::get().unwrap().exception_arg_error(),
                format!(
                    "max_heap_size_mb={mb} overflows when converted to bytes (max: {})",
                    usize::MAX / 1024 / 1024
                ),
            )
        })?;

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

// ---------------------------------------------------------------------------
// Pool initialization (double-checked locking)
// ---------------------------------------------------------------------------

/// Resolves the effective pool size from config.
/// - 0 (default) → auto-detect from CPU count, capped at MAX_ISOLATES
/// - > 0         → as-is, capped at MAX_ISOLATES
fn resolve_pool_size(cfg: Config) -> usize {
    let raw = if cfg.isolate_pool_size > 0 {
        cfg.isolate_pool_size
    } else {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(2)
            .saturating_sub(1) // leave one core for Ruby
    };
    std::cmp::max(1, std::cmp::min(raw, MAX_ISOLATES))
}

// TODO: replace with OnceLock::get_or_try_init once stabilised (tracking issue #109737).
fn get_or_init_pool() -> Result<&'static IsolatePool, Error> {
    if let Some(p) = POOL.get() {
        return Ok(p);
    }
    let _guard = INIT_LOCK.lock().unwrap();
    if let Some(p) = POOL.get() {
        return Ok(p);
    }

    let config = *CONFIG.lock().unwrap();
    let pool_size = resolve_pool_size(config);
    // max_heap_size_mb is a per-isolate V8 CreateParams constraint, NOT a
    // total budget. Each isolate independently gets the configured limit so
    // that workloads calibrated for the single-isolate case don't break when
    // the pool auto-detects more cores. Users with tight memory can reduce
    // the per-isolate limit explicitly.
    let per_isolate_mb = config.max_heap_size_mb;

    let pool = IsolatePool::new(pool_size, per_isolate_mb)
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
    Ok(())
}
