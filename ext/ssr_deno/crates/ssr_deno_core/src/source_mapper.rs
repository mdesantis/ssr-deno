use std::collections::HashMap;
use std::path::Path;
use std::time::SystemTime;

use sourcemap::SourceMap;

/// Self-managed source map registry for SSR bundles.
///
/// Stores parsed source maps keyed by bundle path. Applies IIFE line offset
/// correction (-2 from V8 lines, since the IIFE wrapper adds 2 lines:
/// `(function(){\n` before the bundle and `\n})();` after) when resolving
/// stack frame positions.
///
/// Pure Rust — no V8 dependency. Used in error formatting before errors
/// reach Ruby.
pub struct SsrSourceMapper {
    maps: HashMap<String, (SourceMap, SystemTime)>,
}

impl SsrSourceMapper {
    pub fn new() -> Self {
        Self {
            maps: HashMap::new(),
        }
    }

    /// Register a source map from disk.
    /// Skips if the `.map` file mtime hasn't changed since last registration.
    pub fn register(&mut self, bundle_path: &str, map_path: &Path) {
        let current_mtime = std::fs::metadata(map_path).and_then(|m| m.modified()).ok();

        if let Some((_, cached_mtime)) = self.maps.get(bundle_path) {
            if Some(*cached_mtime) == current_mtime {
                return;
            }
        }

        let Ok(map_data) = std::fs::read(map_path) else {
            return;
        };
        let Ok(sm) = SourceMap::from_slice(&map_data) else {
            return;
        };

        if let Some(mtime) = current_mtime {
            self.maps.insert(bundle_path.to_string(), (sm, mtime));
        }
    }

    /// Resolve V8 stack frame positions to original source locations.
    ///
    /// Processes each line of the error message. Lines matching the V8
    /// stack-frame pattern `at <file>:<line>:<col>` or
    /// `at <func> (<file>:<line>:<col>)` are checked against registered
    /// source maps. The IIFE wrapper offset is corrected:
    /// `sourcemap_line = v8_line - 2` (V8 line 1 = IIFE prefix,
    /// V8 line 2 = bundle line 1 = source map generated index 0).
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
        let (sm, _) = self.maps.get(file_and_path)?;

        // IIFE offset: V8 line 1 = IIFE prefix, V8 line 2 = bundle line 1
        let sm_line = v8_line.saturating_sub(2);
        let sm_col = v8_col.saturating_sub(1);

        let source = sm
            .lookup_token(sm_line, sm_col)
            .and_then(|t| t.get_source())
            .unwrap_or("<unknown>");
        let src_line = sm.lookup_token(sm_line, sm_col).map_or(v8_line, |t| {
            let l = t.get_src_line();
            if l > 0 {
                l + 1
            } else {
                v8_line
            }
        });
        let src_col = sm.lookup_token(sm_line, sm_col).map_or(v8_col, |t| {
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
    fn insert_map(&mut self, bundle_path: &str, sm: SourceMap) {
        self.maps
            .insert(bundle_path.to_string(), (sm, SystemTime::now()));
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
