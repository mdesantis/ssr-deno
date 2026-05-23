use super::*;
use std::io::Write;

// -----------------------------------------------------------------------
// resolve — no map
// -----------------------------------------------------------------------

#[test]
fn resolve_no_map_returns_original() {
    let mapper = SsrSourceMapper::new();
    let msg = "Error: oops\n    at bundle.js:2:9";
    assert_eq!(mapper.resolve(msg), msg);
}

#[test]
fn resolve_empty_message() {
    let mapper = SsrSourceMapper::new();
    assert_eq!(mapper.resolve(""), "");
}

#[test]
fn resolve_non_frame_line_left_alone() {
    let mapper = SsrSourceMapper::new();
    let msg = "Error: something went wrong";
    assert_eq!(mapper.resolve(msg), msg);
}

// -----------------------------------------------------------------------
// resolve — with registered map, IIFE offset
// -----------------------------------------------------------------------

#[test]
fn resolve_iife_offset_corrected() {
    let mut mapper = SsrSourceMapper::new();
    mapper.insert_map("bundle.js", create_test_map());

    // V8 line 3 → bundle line 1 → source map generated line 0
    // V8 line 2 is first bundle line (IIFE offset)
    // Source map maps generated line 0 → original line 0 → src_line 1
    let msg = "Error: test\n    at bundle.js:3:1";
    let resolved = mapper.resolve(msg);

    assert!(resolved.contains("components/thrower.tsx"));
    assert!(!resolved.contains("bundle.js"));
}

#[test]
fn resolve_with_func_name() {
    let mut mapper = SsrSourceMapper::new();
    mapper.insert_map("bundle.js", create_test_map());

    // With function name in parens
    let msg = "Error: test\n    at render (bundle.js:3:1)";
    let resolved = mapper.resolve(msg);

    assert!(resolved.contains("components/thrower.tsx"));
}

// -----------------------------------------------------------------------
// resolve — edge cases
// -----------------------------------------------------------------------

#[test]
fn resolve_map_line_beyond_map_uses_closest_token() {
    let mut mapper = SsrSourceMapper::new();
    mapper.insert_map("bundle.js", create_test_map());

    // Line 999 beyond map — lookup_token returns closest preceding token
    let msg = "Error: test\n    at bundle.js:999:1";
    let resolved = mapper.resolve(msg);

    // Falls back to closest mapping: original source with adjusted line
    assert!(resolved.contains("components/thrower.tsx"));
}

#[test]
fn resolve_unregistered_bundle_left_alone() {
    let mut mapper = SsrSourceMapper::new();
    mapper.insert_map("bundle.js", create_test_map());

    // Different bundle, no map registered
    let msg = "Error: test\n    at other.js:3:1";
    assert_eq!(mapper.resolve(msg), msg);
}

// -----------------------------------------------------------------------
// register — caching
// -----------------------------------------------------------------------

#[test]
fn register_skips_unchanged_map() {
    let dir = std::env::temp_dir().join("ssr_deno_test_source_mapper");
    let _ = std::fs::create_dir_all(&dir);

    let map_path = dir.join("test.js.map");
    let map_content = br#"{
            "version": 3,
            "file": "test.js",
            "sources": ["test.tsx"],
            "mappings": "AAAA"
        }"#;
    let mut file = std::fs::File::create(&map_path).unwrap();
    file.write_all(map_content).unwrap();
    file.flush().unwrap();

    let mut mapper = SsrSourceMapper::new();
    mapper.register("test.js", &map_path);
    assert_eq!(mapper.maps.len(), 1);

    // Register again — same mtime, should skip
    mapper.register("test.js", &map_path);
    assert_eq!(mapper.maps.len(), 1);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn register_missing_map_does_nothing() {
    let mut mapper = SsrSourceMapper::new();
    mapper.register("test.js", Path::new("/nonexistent/missing.js.map"));
    assert!(mapper.maps.is_empty());
}

// -----------------------------------------------------------------------
// register_inline
// -----------------------------------------------------------------------

#[test]
fn register_inline_stores_map() {
    let mut mapper = SsrSourceMapper::new();
    let json = r#"{"version":3,"file":"mod.js","sources":["mod.tsx"],"mappings":"AAAA"}"#;
    mapper.register_inline("/abs/path/mod.tsx", json, SystemTime::now());
    let msg = "Error: oops\n    at /abs/path/mod.tsx:2:1";
    let resolved = mapper.resolve(msg);
    assert!(resolved.contains("mod.tsx"));
}

#[test]
fn register_inline_zero_offset_for_absolute_paths() {
    let mut mapper = SsrSourceMapper::new();
    // Source map mapping line 0 → line 0
    let json = r#"{"version":3,"file":"mod.js","sources":["mod.tsx"],"mappings":"AAAA"}"#;
    mapper.register_inline("/abs/path/mod.tsx", json, SystemTime::now());
    // V8 line 1 = source map line 0 (offset 1 for ESM, no IIFE)
    let msg = "Error: oops\n    at /abs/path/mod.tsx:1:1";
    let resolved = mapper.resolve(msg);
    assert!(resolved.contains("mod.tsx:1:1"));
}

#[test]
fn register_inline_bad_json_does_nothing() {
    let mut mapper = SsrSourceMapper::new();
    mapper.register_inline("/x.tsx", "not-json", SystemTime::now());
    assert_eq!(mapper.resolve("at /x.tsx:1:1"), "at /x.tsx:1:1");
}

// -----------------------------------------------------------------------
// register_inline — different mtime re-registers (line 79 false branch)
// -----------------------------------------------------------------------

#[test]
fn register_inline_updates_when_mtime_changes() {
    let mut mapper = SsrSourceMapper::new();
    let json = r#"{"version":3,"file":"mod.js","sources":["mod.tsx"],"mappings":"AAAA"}"#;
    let mtime1 = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
    mapper.register_inline("/path/mod.tsx", json, mtime1);
    assert_eq!(mapper.maps.len(), 1);

    // Different mtime — must fall through (line 79) and re-register
    let mtime2 = mtime1 + std::time::Duration::from_secs(1);
    mapper.register_inline("/path/mod.tsx", json, mtime2);
    assert_eq!(mapper.maps.len(), 1);
}

// -----------------------------------------------------------------------
// register — mtime changed re-registers (line 49 false branch)
// -----------------------------------------------------------------------

#[test]
fn register_updates_when_mtime_changes() {
    let dir = std::env::temp_dir().join("ssr_deno_test_mtime_update");
    let _ = std::fs::create_dir_all(&dir);
    let map_path = dir.join("changing.js.map");
    let map_json = br#"{"version":3,"file":"t.js","sources":["t.tsx"],"mappings":"AAAA"}"#;
    std::fs::write(&map_path, map_json).unwrap();

    // Pin mtime to a known past time so the cache sees a stale entry on re-register
    let past = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
    std::fs::File::options()
        .write(true)
        .open(&map_path)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(past))
        .unwrap();

    let mut mapper = SsrSourceMapper::new();
    mapper.register("t.js", &map_path);
    assert_eq!(mapper.maps.len(), 1);

    // Advance mtime — forces the false branch of `if Some(entry.mtime) == current_mtime`
    std::fs::File::options()
        .write(true)
        .open(&map_path)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(SystemTime::now()))
        .unwrap();

    mapper.register("t.js", &map_path);
    assert_eq!(mapper.maps.len(), 1);

    let _ = std::fs::remove_dir_all(&dir);
}

// -----------------------------------------------------------------------
// register — file readable but invalid source map (line 56)
// -----------------------------------------------------------------------

#[test]
fn register_invalid_source_map_does_nothing() {
    let dir = std::env::temp_dir().join("ssr_deno_test_bad_map");
    let _ = std::fs::create_dir_all(&dir);
    let map_path = dir.join("bad.js.map");
    std::fs::write(&map_path, b"not-valid-json").unwrap();

    let mut mapper = SsrSourceMapper::new();
    mapper.register("bad.js", &map_path);
    assert!(mapper.maps.is_empty());

    let _ = std::fs::remove_dir_all(&dir);
}

// -----------------------------------------------------------------------
// register_inline — same mtime skips re-parse (lines 77-79)
// -----------------------------------------------------------------------

#[test]
fn register_inline_skips_unchanged_mtime() {
    let mut mapper = SsrSourceMapper::new();
    let json = r#"{"version":3,"file":"mod.js","sources":["mod.tsx"],"mappings":"AAAA"}"#;
    let mtime = SystemTime::now();
    mapper.register_inline("/path/mod.tsx", json, mtime);
    assert_eq!(mapper.maps.len(), 1);

    // Same mtime — must hit the early-return path (lines 77-79)
    mapper.register_inline("/path/mod.tsx", json, mtime);
    assert_eq!(mapper.maps.len(), 1);
}

// -----------------------------------------------------------------------
// resolve — non-zero src_col branch (line 164)
// -----------------------------------------------------------------------

#[test]
fn resolve_nonzero_src_col() {
    // "AAAK" encodes gen_col=0, source=0, orig_line=0, orig_col=5 (VLQ K = +5)
    let json = br#"{"version":3,"file":"bundle.js","sources":["src.tsx"],"mappings":"AAAK"}"#;
    let map = SourceMap::from_slice(json).expect("valid map");
    let mut mapper = SsrSourceMapper::new();
    mapper.insert_map("bundle.js", map);

    // IIFE offset 2: V8 line 2 → sm_line 0, V8 col 1 → sm_col 0
    // token: orig_line=0, orig_col=5 → src_col = 5 + 1 = 6
    let msg = "Error: x\n    at bundle.js:2:1";
    let resolved = mapper.resolve(msg);
    assert!(resolved.contains("src.tsx:2:6"), "got: {resolved}");
}

// -----------------------------------------------------------------------
// Default impl (lines 192-194)
// -----------------------------------------------------------------------

#[test]
fn source_mapper_default() {
    let mapper = SsrSourceMapper::default();
    assert!(mapper.maps.is_empty());
}

// -----------------------------------------------------------------------
// global_get_source_mapper (lines 208-211)
// -----------------------------------------------------------------------

#[test]
fn global_source_mapper_accessible() {
    let guard = global_get_source_mapper().read().unwrap();
    drop(guard);
}

// -----------------------------------------------------------------------
// clear
// -----------------------------------------------------------------------

#[test]
fn clear_removes_all_maps() {
    let mut mapper = SsrSourceMapper::new();
    mapper.insert_map("a.js", create_test_map());
    assert_eq!(mapper.maps.len(), 1);
    mapper.clear();
    assert!(mapper.maps.is_empty());
}
