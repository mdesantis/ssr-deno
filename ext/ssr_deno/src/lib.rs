mod deno_runtime_wrapper;
mod nop_types;
mod sys;

use deno_runtime_wrapper::{DenoError, DenoRuntimeWrapper};
use magnus::{function, Error, ExceptionClass, Module, Object, Ruby};
use std::sync::{Mutex, OnceLock};

static RUNTIME: OnceLock<DenoRuntimeWrapper> = OnceLock::new();
static INIT_LOCK: Mutex<()> = Mutex::new(());
static CONFIG: OnceLock<Config> = OnceLock::new();

/// Configuration passed from Ruby to Rust before runtime initialization.
/// All fields have safe defaults so the runtime can be initialized without
/// calling any setter.
#[derive(Clone, Copy)]
struct Config {
    max_heap_size_mb: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self { max_heap_size_mb: 64 } // 64 MB — sensible for SSR workloads
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

    CONFIG
        .set(Config {
            max_heap_size_mb: mb,
        })
        .map_err(|_| {
            Error::new(
                deno_exc("JsRuntimeInitializationError"),
                "Cannot set config after runtime is already initialized",
            )
        })
}

// TODO: replace with OnceLock::get_or_try_init once stabilised (tracking issue #109737).
fn get_or_init_runtime() -> Result<&'static DenoRuntimeWrapper, Error> {
    if let Some(r) = RUNTIME.get() {
        return Ok(r);
    }
    let _guard = INIT_LOCK.lock().unwrap();
    if let Some(r) = RUNTIME.get() {
        return Ok(r);
    }
    let config = CONFIG.get().copied().unwrap_or_default();
    let rt = DenoRuntimeWrapper::new(config.max_heap_size_mb)
        .map_err(|e| js_runtime_initialization_error(e.to_string()))?;
    let _ = RUNTIME.set(rt);
    Ok(RUNTIME.get().unwrap())
}

fn get_runtime() -> Result<&'static DenoRuntimeWrapper, Error> {
    RUNTIME
        .get()
        .ok_or_else(|| js_runtime_not_initialized_error("Runtime not initialized. Call `SSR::Deno::Bundle.new` first."))
}

/// Loads a bundle into the shared Deno worker, registering its render function
/// under `globalThis.__ssr_bundles[bundle_id]`. Initializes the runtime lazily.
fn native_load_bundle(bundle_id: String, bundle_path: String) -> Result<(), Error> {
    get_or_init_runtime()?
        .load_bundle(&bundle_id, &bundle_path)
        .map_err(|e| js_runtime_initialization_error(e.to_string()))
}

/// Returns the render result as a JSON string so any JS type survives the
/// boundary. Ruby's `JSON.parse` reconstructs the value.
fn native_render(bundle_id: String, args_json: String) -> Result<String, Error> {
    get_runtime()?
        .block_on_render(&bundle_id, &args_json)
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
    // Opt in to Ractor safety. All shared state (RUNTIME) is Rust-level and
    // protected by OnceLock. Renders serialize through a tokio channel so
    // concurrent Ractors queue and get isolated results.
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
    Ok(())
}
