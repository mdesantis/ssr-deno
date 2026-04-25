use magnus::{function, Error, Module, Object, Ruby};

/// A simple hello world function to verify the native extension build pipeline.
/// Returns a greeting string with the provided name.
fn hello(name: String) -> String {
    format!(
        "Hello {}! I'm a Rust native extension running inside Ruby!",
        name
    )
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
    deno_module.define_singleton_method("hello", function!(hello, 1))?;
    deno_module.define_singleton_method("native_version", function!(native_version, 0))?;
    Ok(())
}
