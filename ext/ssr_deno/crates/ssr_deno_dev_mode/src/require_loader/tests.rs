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
