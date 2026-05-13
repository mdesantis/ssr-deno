use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use deno_ast::{
    EmitOptions, MediaType, ParseParams, SourceMapOption, TranspileModuleOptions,
    TranspileOptions,
};
use deno_core::url::Url;
use deno_core::{
    resolve_import, ModuleLoadOptions, ModuleLoadReferrer, ModuleLoadResponse, ModuleLoader,
    ModuleSource, ModuleSourceCode, ModuleSpecifier, ModuleType, ResolutionKind,
};
use deno_error::JsErrorBox;
use deno_resolver::npm::{ByonmInNpmPackageChecker, ByonmNpmResolver};
use node_resolver::{
    cache::NodeResolutionSys, DenoIsBuiltInNodeModuleChecker, NodeConditionOptions,
    NodeResolution, NodeResolutionKind, NodeResolver, NodeResolverOptions, ResolutionMode,
};

use crate::real_npm_types::build_dev_npm_resolver;
use crate::sys::Sys;

pub type SharedAliasMap = Arc<Mutex<Vec<(String, String)>>>;

static EMPTY_JS: &str = "export {};\n";

struct CacheEntry {
    mtime: SystemTime,
    code: String,
    source_map: Option<String>,
}

pub struct DevModuleLoader {
    project_root: PathBuf,
    resolve_alias: SharedAliasMap,
    node_resolver:
        NodeResolver<ByonmInNpmPackageChecker, DenoIsBuiltInNodeModuleChecker, ByonmNpmResolver<Sys>, Sys>,
    cache: Mutex<HashMap<PathBuf, CacheEntry>>,
}

fn resolve_with_ext_fallback(base: &Path) -> Option<PathBuf> {
    if base.exists() {
        return Some(base.to_path_buf());
    }
    for ext in &["ts", "tsx", "js", "jsx"] {
        let candidate = base.with_extension(ext);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

fn is_asset_import(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("css" | "svg" | "png" | "jpg" | "jpeg" | "gif" | "webp" | "ico" | "woff" | "woff2" | "ttf" | "eot")
    )
}

fn needs_transpile(media_type: MediaType) -> bool {
    matches!(
        media_type,
        MediaType::TypeScript
            | MediaType::Tsx
            | MediaType::Jsx
            | MediaType::Mts
            | MediaType::Cts
    )
}

impl DevModuleLoader {
    pub fn new(
        project_root: PathBuf,
        resolve_alias: SharedAliasMap,
    ) -> Self {
        let (npm_checker, npm_resolver, pkg_json_resolver) =
            build_dev_npm_resolver(&project_root);

        let node_resolver = NodeResolver::new(
            npm_checker,
            DenoIsBuiltInNodeModuleChecker,
            npm_resolver,
            pkg_json_resolver,
            NodeResolutionSys::new(Sys, None),
            NodeResolverOptions {
                conditions: NodeConditionOptions {
                    conditions: vec![
                        std::borrow::Cow::Borrowed("node"),
                        std::borrow::Cow::Borrowed("import"),
                    ],
                    import_conditions_override: None,
                    require_conditions_override: None,
                },
                is_browser_platform: false,
                bundle_mode: true,
                typescript_version: None,
            },
        );

        Self {
            project_root,
            resolve_alias,
            node_resolver,
            cache: Mutex::new(HashMap::new()),
        }
    }

    fn resolve_alias_specifier(&self, specifier: &str) -> Option<PathBuf> {
        let guard = self.resolve_alias.lock().ok()?;
        for (prefix, target) in guard.iter() {
            let Some(rest) = specifier.strip_prefix(prefix.as_str()) else {
                continue;
            };
            if !rest.is_empty() && !rest.starts_with('/') {
                continue;
            }
            let rest = rest.strip_prefix('/').unwrap_or(rest);
            let candidate = self.project_root.join(target).join(rest);
            return resolve_with_ext_fallback(&candidate);
        }
        None
    }

    fn resolve_relative_specifier(
        &self,
        specifier: &str,
        referrer: &ModuleSpecifier,
    ) -> Option<PathBuf> {
        let referrer_path = referrer.to_file_path().ok()?;
        let parent = referrer_path.parent()?;
        let candidate = parent.join(specifier);
        resolve_with_ext_fallback(&candidate)
    }

    fn check_cache(&self, path: &Path) -> Option<(String, Option<String>)> {
        let current_mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok()?;
        let cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        let entry = cache.get(path)?;
        if entry.mtime == current_mtime {
            Some((entry.code.clone(), entry.source_map.clone()))
        } else {
            None
        }
    }

    fn update_cache(&self, path: &Path, code: String, source_map: Option<String>) {
        let Ok(mtime) = std::fs::metadata(path).and_then(|m| m.modified()) else {
            return;
        };
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        cache.insert(
            path.to_path_buf(),
            CacheEntry {
                mtime,
                code,
                source_map,
            },
        );
    }

    fn load_and_transpile_source(
        &self,
        path: &Path,
    ) -> Result<(String, Option<String>), JsErrorBox> {
        let source = std::fs::read_to_string(path)
            .map_err(|e| JsErrorBox::generic(format!("Failed to read {}: {e}", path.display())))?;

        let media_type = MediaType::from_path(path);

        if !needs_transpile(media_type) {
            return Ok((source, None));
        }

        let specifier = Url::from_file_path(path)
            .map_err(|_| JsErrorBox::generic(format!("Cannot create file URL for {}", path.display())))?;

        let parsed = deno_ast::parse_module(ParseParams {
            specifier,
            text: source.into(),
            media_type,
            capture_tokens: false,
            scope_analysis: false,
            maybe_syntax: None,
        })
        .map_err(|e| JsErrorBox::generic(format!("Parse error in {}: {e}", path.display())))?;

        let transpiled = parsed
            .transpile(
                &TranspileOptions {
                    imports_not_used_as_values: deno_ast::ImportsNotUsedAsValues::Remove,
                    ..Default::default()
                },
                &TranspileModuleOptions::default(),
                &EmitOptions {
                    source_map: SourceMapOption::Separate,
                    ..Default::default()
                },
            )
            .map_err(|e| {
                JsErrorBox::generic(format!("Transpile error in {}: {e}", path.display()))
            })?
            .into_source();

        Ok((transpiled.text, transpiled.source_map))
    }
}

impl ModuleLoader for DevModuleLoader {
    fn resolve(
        &self,
        specifier: &str,
        referrer: &str,
        _kind: ResolutionKind,
    ) -> Result<ModuleSpecifier, JsErrorBox> {
        if specifier.starts_with("node:") {
            return ModuleSpecifier::parse(specifier).map_err(JsErrorBox::from_err);
        }

        let spec = if let Some(rest) = specifier.strip_prefix("npm:") {
            rest
        } else {
            specifier
        };

        let referrer_url = ModuleSpecifier::parse(referrer).map_err(JsErrorBox::from_err)?;

        if let Some(resolved) = self.resolve_alias_specifier(spec) {
            return Url::from_file_path(&resolved)
                .map_err(|_| JsErrorBox::generic(format!("Cannot create URL for {}", resolved.display())));
        }

        if spec.starts_with("./") || spec.starts_with("../") {
            if let Some(resolved) = self.resolve_relative_specifier(spec, &referrer_url) {
                return Url::from_file_path(&resolved)
                    .map_err(|_| JsErrorBox::generic(format!("Cannot create URL for {}", resolved.display())));
            }
            return resolve_import(specifier, referrer).map_err(JsErrorBox::from_err);
        }

        let resolution = self
            .node_resolver
            .resolve(spec, &referrer_url, ResolutionMode::Import, NodeResolutionKind::Execution)
            .map_err(|e| JsErrorBox::generic(format!("Failed to resolve '{spec}': {e}")))?;

        match resolution {
            NodeResolution::Module(url_or_path) => {
                let url = url_or_path.into_url().map_err(|e| {
                    JsErrorBox::generic(format!("Failed to convert resolution to URL: {e}"))
                })?;
                Ok(url)
            }
            NodeResolution::BuiltIn(name) => {
                ModuleSpecifier::parse(&format!("node:{name}")).map_err(JsErrorBox::from_err)
            }
        }
    }

    fn load(
        &self,
        module_specifier: &ModuleSpecifier,
        _maybe_referrer: Option<&ModuleLoadReferrer>,
        _options: ModuleLoadOptions,
    ) -> ModuleLoadResponse {
        if module_specifier.scheme() == "node" {
            return ModuleLoadResponse::Sync(Err(JsErrorBox::generic(
                "node: modules handled by extension, not by DevModuleLoader",
            )));
        }

        let path = match module_specifier.to_file_path() {
            Ok(p) => p,
            Err(_) => {
                return ModuleLoadResponse::Sync(Err(JsErrorBox::generic(format!(
                    "DevModuleLoader cannot load non-file URL: {module_specifier}"
                ))));
            }
        };

        if is_asset_import(&path) {
            return ModuleLoadResponse::Sync(Ok(ModuleSource::new(
                ModuleType::JavaScript,
                ModuleSourceCode::String(EMPTY_JS.to_string().into()),
                module_specifier,
                None,
            )));
        }

        if let Some((code, source_map)) = self.check_cache(&path) {
            register_source_map(module_specifier, &path, source_map.as_deref());
            return ModuleLoadResponse::Sync(Ok(ModuleSource::new(
                ModuleType::JavaScript,
                ModuleSourceCode::String(code.into()),
                module_specifier,
                None,
            )));
        }

        match self.load_and_transpile_source(&path) {
            Ok((code, source_map)) => {
                register_source_map(module_specifier, &path, source_map.as_deref());
                self.update_cache(&path, code.clone(), source_map);
                ModuleLoadResponse::Sync(Ok(ModuleSource::new(
                    ModuleType::JavaScript,
                    ModuleSourceCode::String(code.into()),
                    module_specifier,
                    None,
                )))
            }
            Err(e) => ModuleLoadResponse::Sync(Err(e)),
        }
    }
}

/// Sort a `HashMap` of aliases by descending prefix length and store in the
/// shared map. Longest-prefix wins at resolve time (Vite/webpack semantics).
/// Called by `dev_load_entry` before each entry load.
pub fn set_aliases(shared: &SharedAliasMap, aliases: &HashMap<String, String>) {
    let mut sorted: Vec<(String, String)> = aliases
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    sorted.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
    let mut guard = shared.lock().unwrap_or_else(|e| e.into_inner());
    *guard = sorted;
}

/// Registers a transpile-produced source map with the global `SsrSourceMapper`
/// so V8 stack frames from transpiled JS resolve back to `.tsx` originals.
///
/// **Keying:** V8 emits stack frames using the module's URL specifier
/// (eg `file:///abs/path/foo.tsx`), not the filesystem path. The mapper key
/// must match what `SsrSourceMapper::resolve_line` extracts from the trace
/// — register under `specifier.as_str()`, not the raw path.
///
/// No-op when the transpile step produced no map (eg `.js` files that
/// `needs_transpile` returned false for) or when the file's mtime can't be
/// read. Best-effort — failure here leaves the trace unmapped, never panics.
fn register_source_map(specifier: &ModuleSpecifier, path: &Path, source_map: Option<&str>) {
    let Some(map_json) = source_map else {
        return;
    };
    let Ok(mtime) = std::fs::metadata(path).and_then(|m| m.modified()) else {
        return;
    };
    let mut mapper = crate::get_source_mapper()
        .write()
        .unwrap_or_else(|e| e.into_inner());
    mapper.register_inline(specifier.as_str(), map_json, mtime);
}
