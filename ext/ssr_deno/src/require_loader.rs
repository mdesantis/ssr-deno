use std::path::Path;

use deno_runtime::deno_core::url::Url;
use deno_runtime::deno_node::NodeRequireLoader;
use deno_runtime::deno_core::FastString;
use deno_error::JsErrorBox;
use node_resolver::errors::PackageJsonLoadError;

/// Minimal [`NodeRequireLoader`] for use with `noExternal: true` bundles.
///
/// All npm dependencies are inlined by Vite, so `require()` is only
/// called for Node.js built-in modules (`stream`, `buffer`, `events`, …).
/// File-system-based loading is rejected — it should never be needed.
#[derive(Debug, Clone)]
pub struct DenoNodeRequireLoader;

impl NodeRequireLoader for DenoNodeRequireLoader {
    fn ensure_read_permission<'a>(
        &self,
        _permissions: &mut deno_runtime::deno_permissions::PermissionsContainer,
        path: std::borrow::Cow<'a, std::path::Path>,
    ) -> Result<std::borrow::Cow<'a, std::path::Path>, JsErrorBox> {
        Ok(path)
    }

    fn load_text_file_lossy(&self, _path: &Path) -> Result<FastString, JsErrorBox> {
        Err(JsErrorBox::generic(
            "File loading via require() is not supported — use noExternal: true",
        ))
    }

    fn is_maybe_cjs(&self, _specifier: &Url) -> Result<bool, PackageJsonLoadError> {
        Ok(false)
    }
}
