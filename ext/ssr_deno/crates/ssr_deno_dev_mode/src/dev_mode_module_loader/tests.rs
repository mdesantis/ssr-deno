use super::*;
use deno_ast::MediaType;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

fn test_tmp_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("ssr_deno_dev_mode_tests");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

// ── is_valid_js_identifier ────────────────────────────────────────────────

#[test]
fn valid_js_identifiers() {
    assert!(is_valid_js_identifier("foo"));
    assert!(is_valid_js_identifier("_bar"));
    assert!(is_valid_js_identifier("$baz"));
    assert!(is_valid_js_identifier("abc123"));
    assert!(is_valid_js_identifier("_"));
    assert!(is_valid_js_identifier("$"));
    // Reserved words are NOT filtered here — just char rules
    assert!(is_valid_js_identifier("class"));
}

#[test]
fn invalid_js_identifiers() {
    assert!(!is_valid_js_identifier(""));
    assert!(!is_valid_js_identifier("123"));
    assert!(!is_valid_js_identifier("foo-bar"));
    assert!(!is_valid_js_identifier("foo bar"));
    assert!(!is_valid_js_identifier("-foo"));
}

// ── is_asset_import ───────────────────────────────────────────────────────

#[test]
fn asset_import_true() {
    assert!(is_asset_import(std::path::Path::new("style.css")));
    assert!(is_asset_import(std::path::Path::new("img.png")));
    assert!(is_asset_import(std::path::Path::new("icon.svg")));
    assert!(is_asset_import(std::path::Path::new("font.woff2")));
    assert!(is_asset_import(std::path::Path::new("favicon.ico")));
}

#[test]
fn asset_import_false() {
    assert!(!is_asset_import(std::path::Path::new("app.tsx")));
    assert!(!is_asset_import(std::path::Path::new("mod.js")));
    assert!(!is_asset_import(std::path::Path::new("data.json")));
}

// ── needs_transpile ───────────────────────────────────────────────────────

#[test]
fn needs_transpile_true() {
    assert!(needs_transpile(MediaType::TypeScript));
    assert!(needs_transpile(MediaType::Tsx));
    assert!(needs_transpile(MediaType::Jsx));
    assert!(needs_transpile(MediaType::Mts));
    assert!(needs_transpile(MediaType::Cts));
}

#[test]
fn needs_transpile_false() {
    assert!(!needs_transpile(MediaType::JavaScript));
    assert!(!needs_transpile(MediaType::Json));
    assert!(!needs_transpile(MediaType::Mjs));
}

// ── looks_like_relative_path ──────────────────────────────────────────────

#[test]
fn relative_path_true() {
    assert!(looks_like_relative_path("./foo"));
    assert!(looks_like_relative_path("../bar"));
    assert!(looks_like_relative_path("/abs"));
}

#[test]
fn relative_path_false() {
    assert!(!looks_like_relative_path("react"));
    assert!(!looks_like_relative_path("@scope/pkg"));
    assert!(!looks_like_relative_path("pkg/sub"));
}

// ── drain_cjs_paths ───────────────────────────────────────────────────────

#[test]
fn drain_cjs_paths_drain_and_empty() {
    let shared: SharedCjsPaths = Arc::new(Mutex::new(Vec::new()));
    {
        let mut guard = shared.lock().unwrap();
        guard.push(std::path::PathBuf::from("/a/b.js"));
        guard.push(std::path::PathBuf::from("/c/d.js"));
    }
    let first = drain_cjs_paths(&shared);
    assert_eq!(first.len(), 2);
    let second = drain_cjs_paths(&shared);
    assert_eq!(second.len(), 0);
}

// ── set_aliases ───────────────────────────────────────────────────────────

#[test]
fn set_aliases_longest_prefix_first() {
    let shared: SharedAliasMap = Arc::new(Mutex::new(Vec::new()));
    let mut aliases = HashMap::new();
    aliases.insert("@/".to_string(), "app/frontend/".to_string());
    aliases.insert(
        "@components/".to_string(),
        "app/frontend/components/".to_string(),
    );
    set_aliases(&shared, aliases);
    let guard = shared.lock().unwrap();
    // Longer prefix ("@components/") must come first
    assert_eq!(guard[0].0, "@components/");
}

// ── DevModeMtimeCache ─────────────────────────────────────────────────────

#[test]
fn mtime_cache_new_is_empty() {
    let cache = DevModeMtimeCache::new();
    let inner = cache.inner.lock().unwrap();
    assert!(inner.is_empty());
}

#[test]
fn mtime_cache_default_works() {
    let _cache = DevModeMtimeCache::default();
}

#[test]
fn mtime_cache_check_nonexistent_is_none() {
    let cache = DevModeMtimeCache::new();
    let result = cache.check(std::path::Path::new("/nonexistent/path/file.ts"));
    assert!(result.is_none());
}

#[test]
fn mtime_cache_update_then_check_returns_code() {
    let tmp = test_tmp_dir();
    let file = tmp.join("mtime_cache_test.ts");
    std::fs::write(&file, "export const x = 1;").unwrap();

    let cache = DevModeMtimeCache::new();
    let code: Arc<str> = Arc::from("export const x = 1;");
    cache.update(&file, code.clone(), None);

    let result = cache.check(&file);
    assert!(result.is_some());
    let (cached_code, cached_map) = result.unwrap();
    assert_eq!(cached_code.as_ref(), code.as_ref());
    assert!(cached_map.is_none());
}

#[test]
fn mtime_cache_any_stale_empty_is_false() {
    let cache = DevModeMtimeCache::new();
    assert!(!cache.any_stale());
}

#[test]
fn mtime_cache_any_stale_current_mtime_is_false() {
    let tmp = test_tmp_dir();
    let file = tmp.join("mtime_stale_current.ts");
    std::fs::write(&file, "export const y = 2;").unwrap();

    let cache = DevModeMtimeCache::new();
    let code: Arc<str> = Arc::from("export const y = 2;");
    cache.update(&file, code, None);

    assert!(!cache.any_stale());
}

#[test]
fn mtime_cache_any_stale_after_file_touch_is_true() {
    let tmp = test_tmp_dir();
    let file = tmp.join("mtime_stale_future.ts");
    std::fs::write(&file, "export const z = 3;").unwrap();

    let cache = DevModeMtimeCache::new();
    let code: Arc<str> = Arc::from("export const z = 3;");
    cache.update(&file, code, None);

    // Advance the file's mtime into the future
    let future = SystemTime::now() + Duration::from_secs(10);
    let times = std::fs::FileTimes::new().set_modified(future);
    std::fs::File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_times(times)
        .unwrap();

    assert!(cache.any_stale());
}

// ── analyze_cjs_exports ───────────────────────────────────────────────────

#[test]
fn analyze_cjs_exports_extracts_names() {
    let tmp = test_tmp_dir();
    let file = tmp.join("cjs_exports_test.js");
    std::fs::write(
        &file,
        "exports.foo = 1; exports.bar = 2; exports.default = 3;\n",
    )
    .unwrap();

    let names = analyze_cjs_exports(&file);
    assert!(
        names.contains(&"foo".to_string()),
        "expected 'foo' in {names:?}"
    );
    assert!(
        names.contains(&"bar".to_string()),
        "expected 'bar' in {names:?}"
    );
    // 'default' is a reserved name and must be excluded
    assert!(
        !names.contains(&"default".to_string()),
        "'default' should be excluded"
    );
}

#[test]
fn analyze_cjs_exports_nonexistent_returns_empty() {
    let names = analyze_cjs_exports(std::path::Path::new("/nonexistent/cjs_file.js"));
    assert!(names.is_empty());
}

// ── looks_like_esm ───────────────────────────────────────────────────────

#[test]
fn looks_like_esm_true_for_esm_file() {
    let tmp = test_tmp_dir();
    let file = tmp.join("esm_detect_true.js");
    std::fs::write(&file, "import React from 'react'; export default React;\n").unwrap();
    assert!(looks_like_esm(&file));
}

#[test]
fn looks_like_esm_false_for_cjs_file() {
    let tmp = test_tmp_dir();
    let file = tmp.join("esm_detect_false.js");
    std::fs::write(&file, "const x = require('x'); module.exports = x;\n").unwrap();
    assert!(!looks_like_esm(&file));
}

#[test]
fn looks_like_esm_false_for_nonexistent() {
    assert!(!looks_like_esm(std::path::Path::new(
        "/nonexistent/esm_file.js"
    )));
}

// ── resolve_cjs_reexport_target ───────────────────────────────────────────

#[test]
fn resolve_cjs_reexport_target_exact_file() {
    let tmp = test_tmp_dir();
    let file = tmp.join("exact_target.js");
    std::fs::write(&file, "module.exports = {};\n").unwrap();

    let result = resolve_cjs_reexport_target(&tmp, "./exact_target.js");
    assert!(result.is_some(), "expected Some for exact file match");
}

#[test]
fn resolve_cjs_reexport_target_extension_fallback() {
    let tmp = test_tmp_dir();
    let file = tmp.join("no_ext_fallback.js");
    std::fs::write(&file, "module.exports = {};\n").unwrap();

    let result = resolve_cjs_reexport_target(&tmp, "./no_ext_fallback");
    assert!(result.is_some(), "expected Some for extension fallback");
}

#[test]
fn resolve_cjs_reexport_target_index_in_dir() {
    let tmp = test_tmp_dir();
    let pkg_dir = tmp.join("cjs_reexport_pkg");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    let index = pkg_dir.join("index.js");
    std::fs::write(&index, "module.exports = {};\n").unwrap();

    let result = resolve_cjs_reexport_target(&tmp, "./cjs_reexport_pkg");
    assert!(result.is_some(), "expected Some for dir/index.js");
}

#[test]
fn resolve_cjs_reexport_target_nonexistent_is_none() {
    let tmp = test_tmp_dir();
    let result = resolve_cjs_reexport_target(&tmp, "./does_not_exist_at_all");
    assert!(result.is_none());
}
