use super::*;

#[test]
fn require_loader_clone_and_debug() {
    let loader = SSRDenoNodeRequireLoader;
    let cloned = loader.clone();
    // Both should format without panicking
    let _ = format!("{loader:?}");
    let _ = format!("{cloned:?}");
}

#[test]
fn load_text_file_lossy_always_errors() {
    // The prod loader never reads from disk — all npm deps are inlined by
    // the bundler. Even an existing file must be rejected.
    let tmp = std::env::temp_dir().join("ssr_deno_require_loader_tests");
    std::fs::create_dir_all(&tmp).unwrap();
    let file = tmp.join("exists.js");
    std::fs::write(&file, "const x = 1;\n").unwrap();

    let loader = SSRDenoNodeRequireLoader;
    let result = loader.load_text_file_lossy(&file);
    assert!(result.is_err());

    let missing = Path::new("/nonexistent/path/that/does/not/exist.js");
    assert!(loader.load_text_file_lossy(missing).is_err());
}

#[test]
fn is_maybe_cjs_always_false() {
    let loader = SSRDenoNodeRequireLoader;
    let url = Url::parse("file:///some/path/mod.js").unwrap();
    assert!(!loader.is_maybe_cjs(&url).unwrap());
}

#[test]
fn is_maybe_cjs_from_require_always_false() {
    let loader = SSRDenoNodeRequireLoader;
    let url = Url::parse("file:///some/path/mod.js").unwrap();
    assert!(!loader.is_maybe_cjs_from_require(&url).unwrap());
}
