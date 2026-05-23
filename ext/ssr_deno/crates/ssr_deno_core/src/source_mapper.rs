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
        let src_line = entry
            .map
            .lookup_token(sm_line, sm_col)
            .map_or(v8_line, |t| {
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

// ---------------------------------------------------------------------------
// Global instance (shared across root and dev-mode crates)
// ---------------------------------------------------------------------------

use std::sync::OnceLock;
use std::sync::RwLock;

/// Returns a reference to the global `SsrSourceMapper` instance.
/// May be called from the root crate (for prod renderers) and from the
/// `ssr_deno_dev_mode` crate (for dev-mode transpilation source-map
/// registration).
pub fn global_get_source_mapper() -> &'static RwLock<SsrSourceMapper> {
    static MAPPER: OnceLock<RwLock<SsrSourceMapper>> = OnceLock::new();
    MAPPER.get_or_init(|| RwLock::new(SsrSourceMapper::new()))
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
mod tests;
