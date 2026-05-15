use std::path::Path;

use deno_error::JsErrorBox;
use deno_runtime::deno_core::url::Url;
use deno_runtime::deno_core::FastString;
use deno_runtime::deno_node::NodeRequireLoader;
use node_resolver::errors::PackageJsonLoadError;

/// Dev-mode [`NodeRequireLoader`] that reads files from disk.
///
/// The synthetic `require()` shim emitted by `DevModeModuleLoader` for npm CJS
/// files dispatches through `globalThis.require("/abs/path")`. That call
/// lands here as `load_text_file_lossy(path)`; we must return the file
/// contents so deno_node's CJS handler can evaluate them. The prod loader
/// (`SSRDenoNodeRequireLoader`) rejects file reads on purpose — dev path
/// can't.
///
/// Read permission is enforced by the worker's `PermissionsContainer`
/// (project root only), so we don't need to re-validate paths here.
#[derive(Debug, Clone)]
pub struct DevModeNodeRequireLoader;

impl NodeRequireLoader for DevModeNodeRequireLoader {
    fn ensure_read_permission<'a>(
        &self,
        _permissions: &mut deno_runtime::deno_permissions::PermissionsContainer,
        path: std::borrow::Cow<'a, std::path::Path>,
    ) -> Result<std::borrow::Cow<'a, std::path::Path>, JsErrorBox> {
        Ok(path)
    }

    fn load_text_file_lossy(&self, path: &Path) -> Result<FastString, JsErrorBox> {
        std::fs::read_to_string(path)
            .map(FastString::from)
            .map_err(|e| {
                JsErrorBox::generic(format!(
                    "Failed to read {} for require(): {e}",
                    path.display()
                ))
            })
    }

    fn is_maybe_cjs(&self, _specifier: &Url) -> Result<bool, PackageJsonLoadError> {
        // Conservative: report every file as possibly-CJS so deno_node's
        // wrapper kicks in. `package.json` "type":"module" still overrides
        // for genuine ESM in node_modules — the flag is a hint, not an
        // assertion.
        Ok(true)
    }
}
