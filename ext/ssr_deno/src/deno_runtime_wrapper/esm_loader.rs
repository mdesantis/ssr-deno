use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use deno_error::JsErrorBox;
use deno_runtime::deno_core::{
    resolve_import, FastString, ModuleLoadOptions, ModuleLoadReferrer, ModuleLoadResponse,
    ModuleLoader, ModuleSource, ModuleSourceCode, ModuleSpecifier, ModuleType, ResolutionKind,
};

// ---------------------------------------------------------------------------
// EsmLoaderState — mutable registry shared between the loader and the worker loop
// ---------------------------------------------------------------------------

/// Per-worker ESM loader state. Tracks allowed directories and synthetic boot
/// modules registered as ESM bundles are loaded. Uses `Rc<RefCell<>>` because
/// the worker thread is single-threaded (one Tokio current-thread runtime).
pub(crate) struct EsmLoaderState {
    allowed_dirs: HashSet<PathBuf>,
    /// bundle_id → (canonical entry path, reload version counter)
    bundles: HashMap<String, (PathBuf, u32)>,
    /// Synthetic module URL → source code (boot modules generated per bundle per version)
    synthetic: HashMap<String, String>,
}

impl Default for EsmLoaderState {
    fn default() -> Self {
        Self {
            allowed_dirs: HashSet::new(),
            bundles: HashMap::new(),
            synthetic: HashMap::new(),
        }
    }
}

impl EsmLoaderState {
    /// Registers a bundle directory and generates a versioned synthetic boot module.
    /// Returns the boot module URL to pass to `preload_main_module`.
    ///
    /// Each call increments the version counter, producing a unique URL that
    /// forces V8 to re-evaluate the boot module and the entry file on reload
    /// (V8 caches modules by specifier URL; unique URLs bypass the cache).
    pub fn register_bundle(&mut self, bundle_id: &str, path: &Path) -> String {
        let entry = self
            .bundles
            .entry(bundle_id.to_owned())
            .or_insert((path.to_owned(), 0));

        entry.0 = path.to_owned();
        entry.1 += 1;

        let version = entry.1;

        if let Some(dir) = path.parent() {
            self.allowed_dirs.insert(dir.to_owned());
        }

        let boot_url = format!("ssr-deno:boot:{}:v={}", bundle_id, version);
        // The ?v=N query forces a unique specifier for the entry file on each
        // reload. FilesystemModuleLoader strips the query when reading from disk.
        let file_url = format!("file://{}?v={}", path.display(), version);
        let bundle_id_js =
            serde_json::to_string(bundle_id).expect("serde_json::to_string cannot fail for &str");

        let source = format!(
            "import {{ render }} from '{file_url}';\n\
             if (typeof globalThis.__ssr_bundles === 'undefined') {{ globalThis.__ssr_bundles = {{}}; }}\n\
             globalThis.__ssr_bundles[{bundle_id_js}] = {{ render }};"
        );

        self.synthetic.insert(boot_url.clone(), source);
        boot_url
    }

    fn is_allowed_path(&self, path: &Path) -> bool {
        self.allowed_dirs.iter().any(|d| path.starts_with(d))
    }

    fn get_synthetic(&self, url: &str) -> Option<String> {
        self.synthetic.get(url).cloned()
    }
}

// ---------------------------------------------------------------------------
// FilesystemModuleLoader — replaces NodeBuiltinOnlyModuleLoader
// ---------------------------------------------------------------------------

/// Module loader that:
/// - Passes `node:` and `ssr-deno:` specifiers through unchanged.
/// - Resolves `file:` specifiers, enforcing that the resolved path stays within
///   a registered bundle directory (security boundary).
/// - Serves synthetic boot modules from `EsmLoaderState`.
/// - Reads JS chunk files from disk for `file:` specifiers.
///
/// For non-ESM bundles (`is_esm: false`), this loader is never consulted
/// (bundles are loaded via `execute_script`, not the ES module system).
pub(crate) struct FilesystemModuleLoader {
    state: Rc<RefCell<EsmLoaderState>>,
}

impl FilesystemModuleLoader {
    pub fn new(state: Rc<RefCell<EsmLoaderState>>) -> Self {
        Self { state }
    }
}

/// Strips the query string from a `ModuleSpecifier` URL so the real filesystem
/// path can be computed. Used to bypass V8's module cache on reload via `?v=N`.
fn strip_query(url: &ModuleSpecifier) -> ModuleSpecifier {
    let mut u = url.clone();
    u.set_query(None);
    u
}

impl ModuleLoader for FilesystemModuleLoader {
    fn resolve(
        &self,
        specifier: &str,
        referrer: &str,
        _kind: ResolutionKind,
    ) -> Result<ModuleSpecifier, JsErrorBox> {
        // node: and ssr-deno: pass through — handled by deno_node extension or
        // synthetic module registry respectively.
        if specifier.starts_with("node:") || specifier.starts_with("ssr-deno:") {
            return ModuleSpecifier::parse(specifier).map_err(JsErrorBox::from_err);
        }

        // Standard URL resolution (handles relative paths, absolute file: URLs, etc.)
        let resolved = resolve_import(specifier, referrer).map_err(JsErrorBox::from_err)?;

        // Security check: file: imports must resolve within a registered bundle dir.
        if resolved.scheme() == "file" {
            let no_query = strip_query(&resolved);
            let path = no_query.to_file_path().map_err(|_| {
                JsErrorBox::generic(format!("Invalid file URL: {resolved}"))
            })?;

            if !self.state.borrow().is_allowed_path(&path) {
                return Err(JsErrorBox::generic(format!(
                    "Import '{}' resolves to '{}' which is outside the bundle directory — \
                     only local chunk imports are supported in ESM bundles",
                    specifier,
                    path.display()
                )));
            }
        }

        Ok(resolved)
    }

    fn load(
        &self,
        module_specifier: &ModuleSpecifier,
        _maybe_referrer: Option<&ModuleLoadReferrer>,
        _options: ModuleLoadOptions,
    ) -> ModuleLoadResponse {
        let url_str = module_specifier.as_str();

        // Serve synthetic boot modules (ssr-deno:boot:bundle-id:v=N).
        if url_str.starts_with("ssr-deno:boot:") {
            let source = self.state.borrow().get_synthetic(url_str);

            return match source {
                Some(code) => ModuleLoadResponse::Sync(Ok(ModuleSource::new(
                    ModuleType::JavaScript,
                    ModuleSourceCode::String(FastString::from(code)),
                    module_specifier,
                    None,
                ))),
                None => ModuleLoadResponse::Sync(Err(JsErrorBox::generic(format!(
                    "Synthetic boot module not found: {url_str}"
                )))),
            };
        }

        // Serve file: scheme from disk (strip ?v=N query before reading).
        if module_specifier.scheme() == "file" {
            let no_query = strip_query(module_specifier);
            let path = match no_query.to_file_path() {
                Ok(p) => p,
                Err(_) => {
                    return ModuleLoadResponse::Sync(Err(JsErrorBox::generic(format!(
                        "Invalid file URL: {module_specifier}"
                    ))));
                }
            };

            return match std::fs::read_to_string(&path) {
                Ok(code) => ModuleLoadResponse::Sync(Ok(ModuleSource::new(
                    ModuleType::JavaScript,
                    ModuleSourceCode::String(FastString::from(code)),
                    module_specifier,
                    None,
                ))),
                Err(e) => ModuleLoadResponse::Sync(Err(JsErrorBox::generic(format!(
                    "Cannot read module '{}': {e}",
                    path.display()
                )))),
            };
        }

        // ext:, https:, etc. — extensions handle their own schemes; the loader
        // should not be called for them. Reject anything we don't own.
        ModuleLoadResponse::Sync(Err(JsErrorBox::generic(format!(
            "Module loading is not supported for scheme '{}' — use a bundler to inline dependencies",
            module_specifier.scheme()
        ))))
    }
}
