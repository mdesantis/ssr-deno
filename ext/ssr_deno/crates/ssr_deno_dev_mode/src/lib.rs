pub mod dev_mode_builder;
pub mod dev_mode_module_loader;
pub mod dev_mode_npm_resolver;
pub mod require_loader;

pub use dev_mode_builder::build_dev_mode_worker;
pub use dev_mode_module_loader::{
    drain_cjs_paths, set_aliases, DevModeModuleLoader, DevModeMtimeCache, SharedAliasMap,
    SharedCjsPaths,
};
pub use dev_mode_npm_resolver::build_dev_mode_npm_resolver;
pub use require_loader::DevModeNodeRequireLoader;

/// Registers the dev-mode FFI methods on the `SSR::Deno` Ruby module.
/// Called from the root crate's `#[magnus::init]`.
pub fn register_dev_mode_ffi(
    ruby: &magnus::Ruby,
    module: magnus::RModule,
) -> Result<(), magnus::Error> {
    // The DevWorkerHandle class and all native_dev_* methods are defined
    // in the root crate (dev_handle, dev_load, dev_worker), where the
    // render engine lives. This function registers dev-mode-only bits
    // that don't need the render engine.
    //
    // Current FFI functions that stay in root:
    //   native_dev_worker_new (creates DevIsolateHandle→spawns dev_worker)
    //   native_dev_render       (dispatches via render engine)
    //   native_dev_render_chunks (dispatches via render engine)
    //   native_dev_check_stale  (queries DevModeMtimeCache)
    //   native_dev_load_entry   (sends LoadEntry to dev worker)
    //
    // If new dev-mode FFI methods are added that don't need the render
    // engine, register them here.
    let _ = (ruby, module);
    Ok(())
}
