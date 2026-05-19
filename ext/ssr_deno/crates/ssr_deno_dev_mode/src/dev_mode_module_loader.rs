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
    cache::NodeResolutionSys, DenoIsBuiltInNodeModuleChecker, NodeResolution, NodeResolutionKind,
    NodeResolver, PackageJsonResolverRc, ResolutionMode,
};

use ssr_deno_sys::Sys;

use crate::dev_mode_npm_resolver::dev_node_resolver_options;

pub type SharedAliasMap = Arc<Mutex<Vec<(String, String)>>>;

/// Paths to every `node_modules/*.{js,cjs}` file the require()-shim is going
/// to wrap. Collected during the load phase; consumed by `dev_load_entry`
/// to pre-populate `globalThis.__cjs_cache` *before* `evaluate_module` runs,
/// so the shim bodies never call `globalThis.require()` from inside V8's
/// module evaluator (the upstream re-entrancy trigger — see
/// `plans/archived/dev-mode-cjs-interop-bug.md`).
pub type SharedCjsPaths = Arc<Mutex<Vec<PathBuf>>>;

/// Drains the collector, returning every CJS path the shim has wrapped so
/// far (in load order). Call this once between `load_main_es_module` and
/// `evaluate_module` to build the warmup script.
pub fn drain_cjs_paths(shared: &SharedCjsPaths) -> Vec<PathBuf> {
    let mut guard = shared.lock().unwrap_or_else(|e| e.into_inner());
    std::mem::take(&mut *guard)
}

static EMPTY_JS: &str = "export {};\n";

struct CacheEntry {
    mtime: SystemTime,
    code: String,
    source_map: Option<String>,
}

pub struct DevModeMtimeCache {
    inner: Mutex<HashMap<PathBuf, CacheEntry>>,
}

impl DevModeMtimeCache {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }
}

impl Default for DevModeMtimeCache {
    fn default() -> Self {
        Self::new()
    }
}

impl DevModeMtimeCache {
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

pub struct DevModeModuleLoader {
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
    cache: Arc<DevModeMtimeCache>,
    /// Every CJS file path the shim wraps gets pushed here during `load()`.
    /// `dev_load_entry` drains this between `load_main_es_module` and
    /// `evaluate_module` to pre-populate `globalThis.__cjs_cache`, so the
    /// shim bodies never call `globalThis.require()` from inside V8's
    /// module evaluator. See `plans/archived/dev-mode-cjs-interop-bug.md`.
    cjs_paths: SharedCjsPaths,
}

fn resolve_with_ext_fallback(base: &Path) -> Option<PathBuf> {
    // Always canonicalize the resolution. Two import paths to the same file
    // (eg `pkg/sub/../impl.mjs` vs `pkg/impl.mjs`) become the same URL,
    // V8's module cache keys collapse, and React context identity is
    // preserved. Without this, MUI's `LocalizationProvider` ran
    // `React.createContext` twice when the same file was reached through a
    // `..` path and a flat path, breaking `useContext` lookups.
    let pick = |candidate: PathBuf| candidate.canonicalize().ok();
    if base.is_file() {
        return pick(base.to_path_buf());
    }
    for ext in &["ts", "tsx", "js", "jsx"] {
        let candidate = base.with_extension(ext);
        if candidate.is_file() {
            return pick(candidate);
        }
    }
    // Directory import — resolve to dir/index.{ts,tsx,js,jsx}
    if base.is_dir() {
        for ext in &["ts", "tsx", "js", "jsx"] {
            let candidate = base.join("index").with_extension(ext);
            if candidate.is_file() {
                return pick(candidate);
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

/// Content-based ESM detection for `.js` files that lack a `"type":"module"`
/// in their nearest `package.json`. Parses the source via `deno_ast` and
/// returns `true` when the program is a `Module` (i.e. has any top-level
/// `import` / `export`), regardless of where the first import/export sits
/// in the source. The first-token sniff was tripping over files like
/// `dom-helpers/esm/removeClass.js` that begin with a plain `function`
/// declaration before reaching the `export default`.
fn looks_like_esm(path: &Path) -> bool {
    let Ok(source) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(specifier) = Url::from_file_path(path) else {
        return false;
    };
    let media_type = match path.extension().and_then(|e| e.to_str()) {
        Some("mjs") => MediaType::Mjs,
        _ => MediaType::JavaScript,
    };
    match deno_ast::parse_program(deno_ast::ParseParams {
        specifier,
        text: std::sync::Arc::<str>::from(source.as_str()),
        media_type,
        capture_tokens: false,
        scope_analysis: false,
        maybe_syntax: None,
    }) {
        Ok(parsed) => matches!(parsed.program_ref(), deno_ast::ProgramRef::Module(_)),
        Err(_) => false,
    }
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

impl DevModeModuleLoader {
    pub fn new(
        project_root: PathBuf,
        resolve_alias: SharedAliasMap,
        cache: Arc<DevModeMtimeCache>,
        cjs_paths: SharedCjsPaths,
        npm_checker: ByonmInNpmPackageChecker,
        npm_resolver: ByonmNpmResolver<Sys>,
        pkg_json_resolver: PackageJsonResolverRc<Sys>,
    ) -> Self {
        let node_resolver = NodeResolver::new(
            npm_checker,
            DenoIsBuiltInNodeModuleChecker,
            npm_resolver.clone(),
            pkg_json_resolver.clone(),
            NodeResolutionSys::new(Sys, None),
            dev_node_resolver_options(),
        );

        let node_modules_dir = project_root.join("node_modules");
        Self {
            project_root,
            node_modules_dir,
            resolve_alias,
            node_resolver,
            pkg_json_resolver,
            cache,
            cjs_paths,
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
                    // Automatic JSX runtime — emits
                    //     import { jsx as _jsx, Fragment as _Fragment } from "react/jsx-runtime";
                    // at the top of each .tsx file instead of `React.createElement(...)`.
                    // Avoids needing `import React from 'react'` in user code (matches
                    // Vite/Rolldown/Next defaults; Vite was secretly injecting React
                    // via `esbuild --inject` in the side-project's prod build).
                    jsx: Some(deno_ast::JsxRuntime::Automatic(
                        deno_ast::JsxAutomaticOptions {
                            development: false,
                            import_source: Some("react".to_string()),
                        },
                    )),
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

impl ModuleLoader for DevModeModuleLoader {
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
        let resolution = match self.node_resolver.resolve(
            spec,
            &referrer_url,
            ResolutionMode::Import,
            NodeResolutionKind::Execution,
        ) {
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
                "node: modules handled by extension, not by DevModeModuleLoader",
            )));
        }

        let path = match module_specifier.to_file_path() {
            Ok(p) => p,
            Err(_) => {
                return ModuleLoadResponse::Sync(Err(JsErrorBox::generic(format!(
                    "DevModeModuleLoader cannot load non-file URL: {module_specifier}"
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

        // node_modules/ CJS files: wrap in a synthetic ESM shim that reads
        // from `globalThis.__cjs_cache` — populated outside the V8 module
        // evaluator by `dev_load_entry` before `evaluate_module` runs. The
        // shim body itself never calls `globalThis.require()`, so the
        // upstream V8 re-entrancy on deep CJS graphs (emotion/MUI/…) can't
        // fire from inside the module-evaluation post-order walk. See
        // `plans/archived/dev-mode-cjs-interop-bug.md`.
        //
        // The shim statically re-exports each CJS export name detected by
        // `analyze_cjs_exports` so ESM consumers can `import { X } from 'pkg'`.
        // Each binding reads `_m.X` at evaluation time — `_m` is the
        // cached require() result, so transitive `module.exports = …` and
        // `Object.defineProperty` cases just work as long as the name made
        // it through static analysis.
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
            // Canonicalize the path before using it as a cache key. The
            // subpackage-fallback (`try_resolve_subpackage`) may return a
            // url containing `..` segments (eg `dom-helpers/addClass/../esm/…`);
            // the warmup script's `require()` collapses the `..` so its
            // cache key is the resolved path, while a separate ESM import
            // of the same file via the resolved path would otherwise hit
            // a different shim entry. Canonicalising here unifies the key.
            let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
            let abs_literal = serde_json::to_string(&canonical.to_string_lossy())
                .expect("serde_json::to_string cannot fail for &str");
            let names = analyze_cjs_exports(&canonical);
            // Record this file in the global collector so `dev_load_entry`
            // can warm `globalThis.__cjs_cache` for it via `execute_script`
            // before V8 starts evaluating the module graph.
            {
                let mut guard = self.cjs_paths.lock().unwrap_or_else(|e| e.into_inner());
                guard.push(canonical.clone());
            }
            let mut shim = format!(
                "const _m = (globalThis.__cjs_cache || {{}})[{abs_literal}];\n\
                 if (_m === undefined) {{\n\
                     throw new Error('CJS module not warmed: ' + {abs_literal});\n\
                 }}\n\
                 export default _m;\n"
            );
            for name in &names {
                use std::fmt::Write as _;
                let _ = writeln!(shim, "export const {name} = _m.{name};");
            }
            // Intentionally NOT cached: `check_cache` returns content
            // without re-running the `cjs_paths.push()` side effect, so a
            // cached shim from a previous worker lifetime would leave the
            // warmup list empty. Re-analysing on every `load_main_es_module`
            // is cheap (single AST walk per file, only called once per
            // worker lifetime).
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
    let mut mapper = ssr_deno_core::source_mapper::global_get_source_mapper()
        .write()
        .unwrap_or_else(|e| e.into_inner());
    mapper.register_inline(specifier.as_str(), map_json, mtime);
}
