use std::collections::HashMap;
use std::path::Path;
use std::time::SystemTime;

use sourcemap::SourceMap;

/// V8 emits 1-indexed line numbers. Subtract the per-entry offset to recover
/// the source map's 0-indexed generated line.
const IIFE_LINE_OFFSET: u32 = 2;
const ESM_LINE_OFFSET: u32 = 1;

struct MapEntry {
    map: SourceMap,
    mtime: SystemTime,
    line_offset: u32,
}

/// Self-managed source map registry for SSR bundles.
///
/// Stores parsed source maps keyed by bundle path (prod) or absolute file
/// path (dev). Each entry stores its own `line_offset`:
///
/// | Source | Offset | Reason |
/// |--------|--------|--------|
/// | Prod IIFE | `IIFE_LINE_OFFSET` (2) | `(function(){\n` before + `\n})()` after |
/// | Dev ESM | `ESM_LINE_OFFSET` (1) | V8 1-indexed → 0-indexed (no wrapper) |
///
/// Pure Rust — no V8 dependency. Used in error formatting before errors
/// reach Ruby.
pub struct SsrSourceMapper {
    maps: HashMap<String, MapEntry>,
}

impl SsrSourceMapper {
    pub fn new() -> Self {
        Self {
            maps: HashMap::new(),
        }
    }

    /// Register a source map from disk (production path, IIFE offset).
    /// Skips if the `.map` file mtime hasn't changed since last registration.
    pub fn register(&mut self, bundle_path: &str, map_path: &Path) {
        let current_mtime = std::fs::metadata(map_path).and_then(|m| m.modified()).ok();

        if let Some(entry) = self.maps.get(bundle_path) {
            if Some(entry.mtime) == current_mtime {
                return;
            }
        }

        let Ok(map_data) = std::fs::read(map_path) else {
            return;
        };
        let Ok(map) = SourceMap::from_slice(&map_data) else {
            return;
        };

        if let Some(mtime) = current_mtime {
            self.maps.insert(
                bundle_path.to_string(),
                MapEntry {
                    map,
                    mtime,
                    line_offset: IIFE_LINE_OFFSET,
                },
            );
        }
    }

    /// Register a source map from an in-memory JSON string (dev path,
    /// ESM offset). Keyed by the absolute file path of the source file.
    /// Skips re-parsing if `mtime` matches the cached entry (parity with
    /// [`register`]).
    pub fn register_inline(&mut self, path: &str, sourcemap_json: &str, mtime: SystemTime) {
        if let Some(entry) = self.maps.get(path) {
            if entry.mtime == mtime {
                return;
            }
        }
        let Ok(map) = SourceMap::from_slice(sourcemap_json.as_bytes()) else {
            return;
        };
        self.maps.insert(
            path.to_string(),
            MapEntry {
                map,
                mtime,
                line_offset: ESM_LINE_OFFSET,
            },
        );
    }

    /// Resolve V8 stack frame positions to original source locations.
    ///
    /// Processes each line of the error message. Lines matching the V8
    /// stack-frame pattern `at <file>:<line>:<col>` or
    /// `at <func> (<file>:<line>:<col>)` are checked against registered
    /// source maps. The stored per-entry `line_offset` is subtracted from
    /// V8's 1-indexed line number to recover the source-map generated line:
    /// `sm_line = v8_line - line_offset` (2 for IIFE-wrapped prod bundles,
    /// 1 for raw ESM dev modules).
    ///
    /// Best-effort — returns original message unchanged on any failure.
    pub fn resolve(&self, msg: &str) -> String {
        msg.lines()
            .map(|line| self.resolve_line(line).unwrap_or_else(|| line.to_string()))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn resolve_line(&self, line: &str) -> Option<String> {
        let trimmed = line.trim();
        if !trimmed.starts_with("at ") {
            return None;
        }

        let after_at = &trimmed[3..];

        // Extract the file:line:col portion.
        // Two formats: "func (file:line:col)" or "file:line:col" (no parens).
        let file_part = if let Some(paren) = after_at.rfind('(') {
            &after_at[paren + 1..]
        } else {
            after_at
        };

        // Remove trailing ")" if present
        let file_part = file_part.trim_end_matches(')');

        // Parse "bundle_name:line:col"
        let (rest, col_str) = file_part.rsplit_once(':')?;
        let (file_and_path, line_str) = rest.rsplit_once(':')?;

        let v8_line: u32 = line_str.parse().ok()?;
        let v8_col: u32 = col_str.parse().ok()?;

        // Check if this file matches a registered bundle
        let entry = self.maps.get(file_and_path)?;

        // Apply the stored line offset (2 for IIFE prod, 1 for ESM dev)
        let sm_line = v8_line.saturating_sub(entry.line_offset);
        let sm_col = v8_col.saturating_sub(1);

        let source = entry
            .map
            .lookup_token(sm_line, sm_col)
            .and_then(|t| t.get_source())
            .unwrap_or("<unknown>");
        let src_line = entry.map.lookup_token(sm_line, sm_col).map_or(v8_line, |t| {
            let l = t.get_src_line();
            if l > 0 {
                l + 1
            } else {
                v8_line
            }
        });
        let src_col = entry.map.lookup_token(sm_line, sm_col).map_or(v8_col, |t| {
            let c = t.get_src_col();
            if c > 0 {
                c + 1
            } else {
                v8_col
            }
        });

        Some(format!("at {source}:{src_line}:{src_col}"))
    }

    /// Remove all registered source maps.
    pub fn clear(&mut self) {
        self.maps.clear();
    }

    #[cfg(test)]
    fn insert_map(&mut self, bundle_path: &str, map: SourceMap) {
        self.maps.insert(
            bundle_path.to_string(),
            MapEntry {
                map,
                mtime: SystemTime::now(),
                line_offset: IIFE_LINE_OFFSET,
            },
        );
    }
}

impl Default for SsrSourceMapper {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// Helpers for test source map creation
// ===========================================================================

#[cfg(test)]
fn create_test_map() -> SourceMap {
    let json = br#"{
        "version": 3,
        "file": "bundle.js",
        "sources": ["components/thrower.tsx"],
        "sourcesContent": ["globalThis.render = function() {\n  throw new Error('test');\n};"],
        "names": [],
        "mappings": "AAAA;AACA"
    }"#;
    SourceMap::from_slice(json).expect("valid test source map")
}

// VLQ "AAAA" = gen_line:0, gen_col:0, source:0, orig_line:0, orig_col:0
// VLQ "AACA" = gen_line:1, gen_col:0, source:0, orig_line:1, orig_col:0
// (relative encoding: each segment is offset from previous)

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
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
}
