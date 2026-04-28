mod deno_runtime_wrapper;
mod nop_types;
mod sys;

use deno_runtime_wrapper::{DenoError, DenoRuntimeWrapper};
use magnus::{function, Error, ExceptionClass, Module, Object, Ruby};
use std::sync::{Mutex, OnceLock};

static RUNTIME: OnceLock<DenoRuntimeWrapper> = OnceLock::new();
static INIT_LOCK: Mutex<()> = Mutex::new(());

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

fn render_error(msg: impl Into<String>) -> Error {
    Error::new(deno_exc("RenderError"), msg.into())
}

/// Initializes the Deno runtime by loading and evaluating the Vite SSR bundle.
///
/// # Arguments
///
/// * `bundle_path` - Path to the self-contained Vite SSR bundle (entry-server.js)
///
/// # Returns
///
/// `true` on first successful initialization, `nil` on subsequent calls.
///
/// # Errors
///
/// Returns `SSR::Deno::JsRuntimeInitializationError` if:
/// - The bundle file cannot be read
/// - The bundle JavaScript cannot be evaluated
fn init_runtime(bundle_path: String) -> Result<Option<bool>, Error> {
    if RUNTIME.get().is_some() {
        return Ok(None);
    }
    let _guard = INIT_LOCK.lock().unwrap();
    if RUNTIME.get().is_some() {
        return Ok(None);
    }
    let runtime = DenoRuntimeWrapper::new(&bundle_path)
        .map_err(|e| js_runtime_initialization_error(e.to_string()))?;
    let _ = RUNTIME.set(runtime);
    Ok(Some(true))
}

/// Renders a component by calling the `render` function in the SSR bundle.
///
/// # Arguments
///
/// * `args_json` - JSON string with `{ component_data, props, url }`
///
/// # Returns
///
/// The rendered HTML string.
///
/// # Errors
///
/// - `SSR::Deno::JsRuntimeNotInitializedError` if `init_runtime` was not called
/// - `SSR::Deno::JsRuntimeWorkerError` if the worker thread died unexpectedly
/// - `SSR::Deno::RenderError` if the JavaScript `render` function throws
fn render(args_json: String) -> Result<String, Error> {
    let runtime = RUNTIME
        .get()
        .ok_or_else(|| js_runtime_not_initialized_error("Runtime not initialized. Call `init_runtime` first."))?;

    runtime.block_on_render(&args_json).map_err(|e| match e {
        DenoError::WorkerDied(msg) => js_runtime_worker_error(msg),
        DenoError::Render(msg) => render_error(msg),
        other => js_runtime_worker_error(other.to_string()),
    })
}

/// Returns the version of the ssr_deno native extension.
fn native_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// The magnus init function — called when Ruby loads the native extension.
/// Registers the `SSR::Deno` module, its exception hierarchy, and its methods.
#[magnus::init]
fn init(ruby: &Ruby) -> Result<(), Error> {
    let module = ruby.define_module("SSR")?;
    let deno_module = module.define_module("Deno")?;

    let base_error = deno_module.define_error("Error", ruby.exception_standard_error())?;
    deno_module.define_error("JsRuntimeInitializationError", base_error)?;
    deno_module.define_error("JsRuntimeNotInitializedError", base_error)?;
    deno_module.define_error("JsRuntimeWorkerError", base_error)?;
    deno_module.define_error("RenderError", base_error)?;

    deno_module.define_singleton_method("init_runtime", function!(init_runtime, 1))?;
    deno_module.define_singleton_method("native_render", function!(render, 1))?;
    deno_module.define_singleton_method("native_version", function!(native_version, 0))?;
    Ok(())
}
