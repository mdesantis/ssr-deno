mod deno_runtime_wrapper;
mod nop_types;
mod sys;

use deno_runtime_wrapper::DenoRuntimeWrapper;
use magnus::{function, Error, Module, Object, Ruby};
use std::sync::{Mutex, OnceLock};

static RUNTIME: OnceLock<DenoRuntimeWrapper> = OnceLock::new();
static INIT_LOCK: Mutex<()> = Mutex::new(());

/// Helper to create a Ruby runtime error using the current Ruby instance.
fn runtime_error(msg: impl Into<String>) -> Error {
    Error::new(Ruby::get().unwrap().exception_runtime_error(), msg.into())
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
/// Returns an error if:
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
        .map_err(|e| runtime_error(format!("Failed to initialize runtime: {e}")))?;
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
/// Returns an error if:
/// - The runtime has not been initialized (call `init_runtime` first)
/// - The JavaScript `render` function throws an error
/// - The render result cannot be converted to a string
fn render(args_json: String) -> Result<String, Error> {
    let runtime = RUNTIME
        .get()
        .ok_or_else(|| runtime_error("Runtime not initialized. Call `init_runtime` first."))?;

    runtime
        .block_on_render(&args_json)
        .map_err(|e| runtime_error(format!("Render failed: {e}")))
}

/// Returns the version of the ssr_deno native extension.
fn native_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// The magnus init function — called when Ruby loads the native extension.
/// Registers the `SSR::Deno` module and its methods.
#[magnus::init]
fn init(ruby: &Ruby) -> Result<(), Error> {
    let module = ruby.define_module("SSR")?;
    let deno_module = module.define_module("Deno")?;
    deno_module.define_singleton_method("init_runtime", function!(init_runtime, 1))?;
    deno_module.define_singleton_method("native_render", function!(render, 1))?;
    deno_module.define_singleton_method("native_version", function!(native_version, 0))?;
    Ok(())
}
