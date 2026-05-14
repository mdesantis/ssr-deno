use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use deno_ast::{
    EmitOptions, MediaType, ParseParams, SourceMapOption, TranspileModuleOptions, TranspileOptions,
};
use deno_core::url::Url;
use deno_core::{
    resolve_import, FastString, ModuleLoadOptions, ModuleLoadReferrer, ModuleLoadResponse,
    ModuleLoader, ModuleSource, ModuleSourceCode, ModuleSpecifier, ModuleType, ResolutionKind,
};
use deno_error::JsErrorBox;
use deno_resolver::npm::{ByonmInNpmPackageChecker, ByonmNpmResolver};
use node_resolver::{
    cache::NodeResolutionSys, DenoIsBuiltInNodeModuleChecker, NodeConditionOptions, NodeResolution,
    NodeResolutionKind, NodeResolver, NodeResolverOptions, PackageJsonResolverRc, ResolutionMode,
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

pub struct DevMtimeCache {
    inner: Mutex<HashMap<PathBuf, CacheEntry>>,
}

impl DevMtimeCache {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    pub fn any_stale(&self) -> bool {
        let snapshot: Vec<(PathBuf, SystemTime)> = {
            let cache = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            cache.iter().map(|(p, e)| (p.clone(), e.mtime)).collect()
        };
        snapshot.into_iter().any(|(path, cached_mtime)| {
            std::fs::metadata(&path)
                .and_then(|m| m.modified())
                .map_or(true, |current| current != cached_mtime)
        })
    }

    fn check(&self, path: &Path) -> Option<(String, Option<String>)> {
        let current_mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok()?;
        let cache = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let entry = cache.get(path)?;
        if entry.mtime == current_mtime {
            Some((entry.code.clone(), entry.source_map.clone()))
        } else {
            None
        }
    }

    fn update(&self, path: &Path, code: String, source_map: Option<String>) {
        let Ok(mtime) = std::fs::metadata(path).and_then(|m| m.modified()) else {
            return;
        };
        let mut cache = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        cache.insert(
            path.to_path_buf(),
            CacheEntry {
                mtime,
                code,
                source_map,
            },
        );
    }
}

pub struct DevModuleLoader {
    project_root: PathBuf,
    // Precomputed `project_root.join("node_modules")` — avoids allocating a
    // PathBuf on every `load()` call when discriminating npm vs project source.
    node_modules_dir: PathBuf,
    resolve_alias: SharedAliasMap,
    node_resolver: NodeResolver<
        ByonmInNpmPackageChecker,
        DenoIsBuiltInNodeModuleChecker,
        ByonmNpmResolver<Sys>,
        Sys,
    >,
    /// Cached `PackageJsonResolver` for querying the nearest `package.json`
    /// `type` field — used to decide whether a `node_modules/*.js` file is
    /// ESM (`"type": "module"`) and should skip the require() shim.
    pkg_json_resolver: PackageJsonResolverRc<Sys>,
    cache: Arc<DevMtimeCache>,
}

fn resolve_with_ext_fallback(base: &Path) -> Option<PathBuf> {
    if base.is_file() {
        return Some(base.to_path_buf());
    }
    for ext in &["ts", "tsx", "js", "jsx"] {
        let candidate = base.with_extension(ext);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    // Directory import — resolve to dir/index.{ts,tsx,js,jsx}
    if base.is_dir() {
        for ext in &["ts", "tsx", "js", "jsx"] {
            let candidate = base.join("index").with_extension(ext);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn is_asset_import(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some(
            "css"
                | "svg"
                | "png"
                | "jpg"
                | "jpeg"
                | "gif"
                | "webp"
                | "ico"
                | "woff"
                | "woff2"
                | "ttf"
                | "eot"
        )
    )
}

fn needs_transpile(media_type: MediaType) -> bool {
    matches!(
        media_type,
        MediaType::TypeScript | MediaType::Tsx | MediaType::Jsx | MediaType::Mts | MediaType::Cts
    )
}

/// Quick content-based ESM detection for `.js` files that lack a
/// `"type":"module"` in their nearest `package.json`. Reads the file,
/// strips leading whitespace and `"use strict"`, then checks whether the
/// first token is `import` or `export`.  Covers packages that ship ESM
/// via `"module"` field without setting `"type"` (e.g. `react-transition-group`).
fn looks_like_esm(path: &Path) -> bool {
    let Ok(source) = std::fs::read_to_string(path) else {
        return false;
    };
    let trimmed = source.trim_start();
    for prefix in ["'use strict';", "\"use strict\";"] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            let rest = rest.trim_start();
            if rest.starts_with("import ") || rest.starts_with("export ") {
                return true;
            }
        }
    }
    trimmed.starts_with("import ") || trimmed.starts_with("export ")
}

/// JS identifier rules (subset of the full Unicode spec — good enough for
/// the names found in `exports.X = ...` patterns inside npm CJS sources).
/// Reserved word filtering is handled by the caller's explicit deny-list
/// because `swc` lets `exports.default = ...` through here as the literal
/// string "default" — we'd reject it via `RESERVED_NAMES`, not this check.
fn is_valid_js_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' || c == '$' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

/// Names that are syntactically valid identifiers but cannot appear in
/// `export const NAME = ...`. `default` is handled separately by the shim.
/// The rest are JS reserved words that occasionally show up as CJS export
/// names (eg `exports.delete = …` in attribute-style APIs).
const RESERVED_NAMES: &[&str] = &[
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "function",
    "if",
    "import",
    "in",
    "instanceof",
    "let",
    "new",
    "null",
    "return",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "with",
    "yield",
];

/// Statically analyses a CJS source for its exported names so the shim can
/// re-expose them as ESM named exports. Returns names that are safe to use
/// in `export const NAME = _m.NAME;`.
///
/// Uses `deno_ast::analyze_cjs` (the same routine that `cjs-module-lexer`
/// implements) — a pure AST walk, never invokes V8, so it sidesteps the
/// upstream re-entrancy bug that blocked the full `NodeCodeTranslator` path.
///
/// Recurses through `module.exports = require('./X')` style re-exports
/// (React, MUI, emotion all hide their real exports behind a NODE_ENV
/// branched indirection). `MAX_REEXPORT_DEPTH` caps recursion at a safe
/// distance — typical depth is 1–2; the limit only guards against runaway
/// cycles in pathologically authored packages.
fn analyze_cjs_exports(path: &Path) -> Vec<String> {
    const MAX_REEXPORT_DEPTH: u8 = 6;
    let mut visited: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    let mut out: Vec<String> = Vec::new();
    collect_cjs_exports(path, MAX_REEXPORT_DEPTH, &mut visited, &mut out);
    out.sort();
    out.dedup();
    out.retain(|n| {
        is_valid_js_identifier(n) && !RESERVED_NAMES.contains(&n.as_str()) && n != "__esModule"
    });
    out
}

fn collect_cjs_exports(
    path: &Path,
    depth: u8,
    visited: &mut std::collections::HashSet<PathBuf>,
    out: &mut Vec<String>,
) {
    if depth == 0 {
        return;
    }
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if !visited.insert(canonical) {
        return;
    }

    let Ok(source) = std::fs::read_to_string(path) else {
        return;
    };
    let Ok(specifier) = Url::from_file_path(path) else {
        return;
    };
    let media_type = match path.extension().and_then(|e| e.to_str()) {
        Some("cjs") => MediaType::Cjs,
        _ => MediaType::JavaScript,
    };
    let parsed = match deno_ast::parse_program(deno_ast::ParseParams {
        specifier,
        text: std::sync::Arc::<str>::from(source.as_str()),
        media_type,
        capture_tokens: false,
        scope_analysis: false,
        maybe_syntax: None,
    }) {
        Ok(p) => p,
        Err(_) => return,
    };
    let analysis = parsed.analyze_cjs();
    out.extend(analysis.exports);

    let parent = path.parent();
    for reexp in analysis.reexports {
        if looks_like_relative_path(&reexp) {
            if let Some(target) = parent.and_then(|dir| resolve_cjs_reexport_target(dir, &reexp)) {
                collect_cjs_exports(&target, depth - 1, visited, out);
            }
        } else {
            // Bare name (eg `exports.x = require('pkg').x`) — keep it; the
            // shim's runtime `_m.x` lookup will find it if the package
            // re-exports correctly. Cross-package recursion is out of scope.
            out.push(reexp);
        }
    }
}

fn looks_like_relative_path(spec: &str) -> bool {
    spec.starts_with("./") || spec.starts_with("../") || spec.starts_with('/')
}

fn resolve_cjs_reexport_target(referrer_dir: &Path, spec: &str) -> Option<PathBuf> {
    let candidate = referrer_dir.join(spec);
    if candidate.is_file() {
        return Some(candidate);
    }
    for ext in &["js", "cjs"] {
        let cand = candidate.with_extension(ext);
        if cand.is_file() {
            return Some(cand);
        }
    }
    if candidate.is_dir() {
        for ext in &["js", "cjs"] {
            let cand = candidate.join("index").with_extension(ext);
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    None
}

impl DevModuleLoader {
    pub fn new(
        project_root: PathBuf,
        resolve_alias: SharedAliasMap,
        cache: Arc<DevMtimeCache>,
    ) -> Self {
        let (npm_checker, npm_resolver, pkg_json_resolver) = build_dev_npm_resolver(&project_root);

        let node_resolver = NodeResolver::new(
            npm_checker,
            DenoIsBuiltInNodeModuleChecker,
            npm_resolver.clone(),
            pkg_json_resolver.clone(),
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

        let node_modules_dir = project_root.join("node_modules");
        Self {
            project_root,
            node_modules_dir,
            resolve_alias,
            node_resolver,
            pkg_json_resolver,
            cache,
        }
    }

    fn resolve_alias_specifier(&self, specifier: &str) -> Option<PathBuf> {
        let guard = self.resolve_alias.lock().unwrap_or_else(|e| e.into_inner());
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
        self.cache.check(path)
    }

    fn update_cache(&self, path: &Path, code: String, source_map: Option<String>) {
        self.cache.update(path, code, source_map)
    }

    /// Returns `true` when `path` is inside a `node_modules` package whose
    /// nearest `package.json` declares `"type": "module"`.  For those files
    /// the require() shim is wrong — they are genuine ESM and must be
    /// loaded directly by V8.
    fn is_esm_inside_node_modules(&self, path: &Path) -> bool {
        self.pkg_json_resolver
            .get_closest_package_json(path)
            .ok()
            .flatten()
            .map(|pkg| pkg.typ == "module")
            .unwrap_or(false)
            || looks_like_esm(path)
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

        let specifier = Url::from_file_path(path).map_err(|_| {
            JsErrorBox::generic(format!("Cannot create file URL for {}", path.display()))
        })?;

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

    /// Fallback for subpackage patterns that [`NodeResolver`] can't handle.
    ///
    /// Packages like `dom-helpers` ship each API surface (`addClass`,
    /// `removeClass`, …) as a directory with its own `package.json` that
    /// redirects via `"module": "../esm/addClass.js"`.  The NodeResolver's
    /// path-traversal guard treats the `../` prefix as escaping the package
    /// boundary and rejects the resolution.
    ///
    /// This method walks the subpath directories manually, reads the
    /// terminal `package.json`, resolves `module` (preferred) or `main`
    /// relative to that directory, canonicalizes the result, and returns
    /// the file URL.  Returns `None` when the spec does not match the
    /// subpackage pattern or any step fails.
    fn try_resolve_subpackage(&self, spec: &str) -> Option<ModuleSpecifier> {
        if spec.starts_with('.') || spec.starts_with('/') {
            return None;
        }

        // Split into package name and subpath.
        // Scoped packages: find second '/'.
        let slash_pos = spec.find('/')?;
        let (pkg_name, subpath) = if spec.starts_with('@') {
            let second = spec[slash_pos + 1..].find('/')?;
            let split = slash_pos + 1 + second;
            (&spec[..split], &spec[split + 1..])
        } else {
            (&spec[..slash_pos], &spec[slash_pos + 1..])
        };

        let pkg_dir = self.node_modules_dir.join(pkg_name);
        if !pkg_dir.is_dir() {
            return None;
        }

        let mut target = pkg_dir.join(subpath);

        if target.is_dir() {
            if let Ok(Some(pkg)) = self
                .pkg_json_resolver
                .load_package_json(&target.join("package.json"))
            {
                let entry = pkg.module.as_deref().or(pkg.main.as_deref())?;
                target = target.join(entry);
            }
        }

        let canonical = std::path::absolute(&target).ok()?;

        let resolved = resolve_with_ext_fallback(&canonical)?;

        if !resolved.starts_with(&self.node_modules_dir) || !resolved.is_file() {
            return None;
        }

        Url::from_file_path(&resolved).ok()
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

        // Resolve the referrer to a URL. The referrer may be "." for the
        // main module — fall back to the project root.
        let referrer_url: ModuleSpecifier = match resolve_import(referrer, "file:///dev/null") {
            Ok(url) => url,
            Err(_) => Url::from_file_path(&self.project_root)
                .map_err(|()| JsErrorBox::generic("cannot resolve referrer"))?,
        };

        // Alias resolution (e.g. @/ → app/frontend/)
        if let Some(resolved) = self.resolve_alias_specifier(spec) {
            return Url::from_file_path(&resolved).map_err(|_| {
                JsErrorBox::generic(format!("Cannot create URL for {}", resolved.display()))
            });
        }

        // Relative paths
        if spec.starts_with("./") || spec.starts_with("../") {
            if let Some(resolved) = self.resolve_relative_specifier(spec, &referrer_url) {
                return Url::from_file_path(&resolved).map_err(|_| {
                    JsErrorBox::generic(format!("Cannot create URL for {}", resolved.display()))
                });
            }
            return resolve_import(specifier, referrer).map_err(JsErrorBox::from_err);
        }

        // Bare specifier — use NodeResolver (walks node_modules/)
        let resolution = match self
            .node_resolver
            .resolve(
                spec,
                &referrer_url,
                ResolutionMode::Import,
                NodeResolutionKind::Execution,
            )
        {
            Ok(r) => r,
            Err(e) => {
                // Subpackage fallback: some packages (dom-helpers, …) ship
                // subdirs with their own package.json whose module/main
                // field uses ../ to reach sibling dirs — the NodeResolver's
                // path-traversal guard rejects these.
                if let Some(url) = self.try_resolve_subpackage(spec) {
                    return Ok(url);
                }
                return Err(JsErrorBox::generic(format!(
                    "Failed to resolve '{spec}': {e}"
                )));
            }
        };

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
                ModuleSourceCode::String(FastString::from_static(EMPTY_JS)),
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

        // node_modules/ CJS files: wrap in a synthetic ESM shim that loads
        // via globalThis.require (set up by setup_require, see
        // deno_runtime_wrapper/worker.rs). This avoids Deno's native CJS→ESM
        // interop which triggers V8 re-entrancy on deep require() graphs
        // (emotion/MUI etc.) — see plans/dev-mode-cjs-interop-bug.md.
        //
        // The shim statically re-exports each CJS export name detected by
        // `analyze_cjs_exports` so ESM consumers can `import { X } from 'pkg'`.
        // Each binding reads `_m.X` at evaluation time — `_m` is the require()
        // result, so transitive `module.exports = …` and `Object.defineProperty`
        // cases just work as long as the name made it through static analysis.
        //
        // **ESM .js files are excluded** — `package.json` `"type":"module"`
        // (or an explicit `exports.import` condition) means the file is genuine
        // ESM and must be loaded directly by V8, not wrapped in require().
        // `.mjs` is always ESM and naturally falls through.
        if path.starts_with(&self.node_modules_dir)
            && path.extension().is_some_and(|e| {
                e == "cjs" || (e == "js" && !self.is_esm_inside_node_modules(&path))
            })
        {
            let abs_literal = serde_json::to_string(&path.to_string_lossy())
                .expect("serde_json::to_string cannot fail for &str");
            let names = analyze_cjs_exports(&path);
            let mut shim =
                format!("const _m = globalThis.require({abs_literal});\nexport default _m;\n");
            for name in &names {
                use std::fmt::Write as _;
                let _ = writeln!(shim, "export const {name} = _m.{name};");
            }
            self.update_cache(&path, shim.clone(), None);
            return ModuleLoadResponse::Sync(Ok(ModuleSource::new(
                ModuleType::JavaScript,
                ModuleSourceCode::String(shim.into()),
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
    sorted.sort_by_key(|b| std::cmp::Reverse(b.0.len()));
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
