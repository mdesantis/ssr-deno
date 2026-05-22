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

#[cfg(test)]
mod tests {
    use super::*;
    use deno_runtime::deno_core::url::Url;

    #[test]
    fn require_loader_clone_and_debug() {
        let loader = DevModeNodeRequireLoader;
        let cloned = loader.clone();
        // Both should format without panicking
        let _ = format!("{loader:?}");
        let _ = format!("{cloned:?}");
    }

    #[test]
    fn is_maybe_cjs_always_true() {
        let loader = DevModeNodeRequireLoader;
        let url = Url::parse("file:///some/path/mod.js").unwrap();
        assert!(loader.is_maybe_cjs(&url).unwrap());
        let url2 = Url::parse("file:///other/path/index.mjs").unwrap();
        assert!(loader.is_maybe_cjs(&url2).unwrap());
    }

    #[test]
    fn load_text_file_lossy_reads_file() {
        let tmp = std::env::temp_dir().join("ssr_deno_require_loader_tests");
        std::fs::create_dir_all(&tmp).unwrap();
        let file = tmp.join("test_read.js");
        std::fs::write(&file, "const x = 1;\n").unwrap();

        let loader = DevModeNodeRequireLoader;
        let result = loader.load_text_file_lossy(&file);
        assert!(result.is_ok());
        let content = result.unwrap();
        assert!(content.as_str().contains("const x = 1;"));
    }

    #[test]
    fn load_text_file_lossy_nonexistent_errors() {
        let loader = DevModeNodeRequireLoader;
        let path = std::path::Path::new("/nonexistent/path/that/does/not/exist.js");
        let result = loader.load_text_file_lossy(path);
        assert!(result.is_err());
    }
}
