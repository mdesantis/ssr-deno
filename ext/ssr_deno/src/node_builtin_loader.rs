use deno_error::JsErrorBox;
use deno_runtime::deno_core::{
    resolve_import, ModuleLoadOptions, ModuleLoadReferrer, ModuleLoadResponse, ModuleLoader,
    ModuleSpecifier, ResolutionKind,
};

/// Module loader that only allows `node:` scheme URLs.
///
/// This replaces [`deno_core::NoopModuleLoader`] so that the `deno_node`
/// extension's built-in polyfills (e.g. `node:module` → `01_require.js`)
/// can be loaded. Non-node: specifiers are still rejected, preserving
/// the security model.
#[derive(Debug, Clone)]
pub struct NodeBuiltinOnlyModuleLoader;

impl ModuleLoader for NodeBuiltinOnlyModuleLoader {
    fn resolve(
        &self,
        specifier: &str,
        referrer: &str,
        _kind: ResolutionKind,
    ) -> Result<ModuleSpecifier, JsErrorBox> {
        if specifier.starts_with("node:") {
            return ModuleSpecifier::parse(specifier).map_err(JsErrorBox::from_err);
        }
        // Allow relative/absolute resolution (used by polyfill internal imports)
        resolve_import(specifier, referrer).map_err(JsErrorBox::from_err)
    }

    fn load(
        &self,
        _module_specifier: &ModuleSpecifier,
        _maybe_referrer: Option<&ModuleLoadReferrer>,
        _options: ModuleLoadOptions,
    ) -> ModuleLoadResponse {
        // The deno_node extension registers its polyfills via Extension::esm,
        // which serves the source code directly — the loader is never asked
        // to load them. If we reach here, the module is not one we support.
        ModuleLoadResponse::Sync(Err(JsErrorBox::generic(
            "Module loading via loader is not supported — only node: scheme allowed",
        )))
    }
}
