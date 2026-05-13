# SSR source map support

Enable V8 stack traces to resolve to original `.tsx`/`.ts` source files.
Opt-in via global config, on by default in non-production Rails envs.

## Problem

Errors from SSR bundles show minified positions:
```
TypeError: null
  at ssr.js:1:4321
```

With source maps:
```
TypeError: null
  at createEmotionCache (app/frontend/lib/create_emotion_cache.ts:17)
```

## Architecture

Bundles emitted by Rolldown include `.js.map` sidecars. ssr-deno loads bundles
via `execute_script()` + IIFE wrapper, so Deno's auto source map resolution
never triggers. Instead of patching deno_core internals, we manage our own
source map registry in `ssr_deno_core` and apply resolution when formatting
error messages.

```mermaid
flowchart LR
  A[Bundle .js] -->|Rolldown| B[Bundle .js + .js.map]
  B -->|native_load_bundle| C[load_bundle_in_worker]
  C -->|source_maps enabled?| D[Read .js.map from disk]
  D -->|parse| E[SsrSourceMapper.register]
  E -->|error occurs in render| F[resolve_stack_message]
  F -->|lookup bundle_path + adjust IIFE offset| G[format original position]
  G -->|Stack trace| H["file.ts: line:col"]
```

**Key difference from deno_core's SourceMapper:** we don't touch V8 internals.
Source map resolution happens purely in Rust, on the error string returned by
V8, before the error reaches Ruby.

## IIFE line offset

Bundles are wrapped in `(function(){\n...\n})();` at `worker.rs:213`. This
shifts all bundle lines by +1 relative to V8's reported positions.

```
V8 reports line 5 → actual bundle line is 4 → source map lookup uses line 4
```

Resolution adjusts: `bundle_line = v8_line - 1` before source map lookup.

## Self-managed source map registry — no deno_core patching

Avoids `build.rs` registry hacks. `SsrSourceMapper` lives in `ssr_deno_core`
(the pure-Rust crate with no V8 dep) so it's fast to compile and testable with
`cargo test -p ssr_deno_core`.

```rust
// crates/ssr_deno_core/src/source_mapper.rs
use std::collections::HashMap;
use std::path::Path;
use std::time::SystemTime;

pub struct SsrSourceMapper {
    // bundle_path → (parsed source map, .map file mtime)
    maps: HashMap<String, (sourcemap::SourceMap, SystemTime)>,
}

impl SsrSourceMapper {
    pub fn register(&mut self, bundle_path: &str, map_path: &Path) { ... }
    pub fn resolve(&self, msg: &str) -> String { ... }
    pub fn clear(&mut self) { ... }
}
```

The `resolve` method parses the V8 error string, finds `at <script_name>:<line>:<col>`
patterns, adjusts for IIFE offset, looks up the source map for `<script_name>`,
and replaces the position with the original source location.

### Sourcemap crate

Add to `ext/ssr_deno/Cargo.toml`:

```toml
sourcemap = "9"
```

And to `crates/ssr_deno_core/Cargo.toml` (where `SsrSourceMapper` lives):

```toml
[dependencies]
sourcemap = "9"
```

No V8 dep. Pure Rust. Compiles in seconds.

### .map file caching

`register()` stores the parsed `SourceMap` keyed by bundle path.
`register()` is called on every `load_bundle_in_worker` (including reloads).
It compares the current `.map` mtime with the cached mtime — skips parsing
if unchanged. This avoids re-parsing multi-MB `.map` files on every reload.

```rust
pub fn register(&mut self, bundle_path: &str, map_path: &Path) {
    let current_mtime = fs::metadata(map_path).and_then(|m| m.modified()).ok();
    if let Some((_, cached_mtime)) = self.maps.get(bundle_path) {
        if Some(*cached_mtime) == current_mtime {
            return; // already cached, unchanged
        }
    }
    // read + parse + store
}
```

## Files

### Rust layer — ssr_deno_core (pure Rust, no V8)

| File | Change |
|---|---|
| `crates/ssr_deno_core/Cargo.toml` | Add `sourcemap = "9"` dep |
| `crates/ssr_deno_core/src/lib.rs` | Add `source_maps: bool` to `Config` (default `false`) |
| `crates/ssr_deno_core/src/source_mapper.rs` | **New** — `SsrSourceMapper` struct |

### Rust layer — ssr_deno (main crate, V8)

| File | Change |
|---|---|
| `ext/ssr_deno/Cargo.toml` | Add `sourcemap = "9"` dep |
| `ext/ssr_deno/src/deno_runtime_wrapper/types.rs` | Add `source_maps: bool` to `WorkerMsg::LoadBundle` |
| `ext/ssr_deno/src/deno_runtime_wrapper/pool.rs` | Accept `source_maps` in `new()`, pass in `WorkerMsg` |
| `ext/ssr_deno/src/deno_runtime_wrapper/handle.rs` | Thread `source_maps` from config/pool to worker |
| `ext/ssr_deno/src/deno_runtime_wrapper/worker.rs` | In `load_bundle_in_worker`: if `source_maps`, read `.js.map` and register with global `SSR_SOURCE_MAPPER` |
| `ext/ssr_deno/src/deno_runtime_wrapper/render.rs` | On error from V8: pass through `SSR_SOURCE_MAPPER.resolve()` before returning |
| `ext/ssr_deno/src/lib.rs` | Add `native_set_source_maps_enabled` / `native_get_source_maps_enabled` FFI, pass `config.source_maps` to pool |

### Ruby layer

| File | Change |
|---|---|
| `lib/ssr/deno.rb` | Require new config files |
| `lib/ssr/deno/config.rb` | Add `source_maps_enabled=` setter + getter + `SSR_DENO_SOURCE_MAPS_ENABLED` env var |
| `lib/ssr/deno/rails/railtie.rb` | Default: `config.ssr_deno.source_maps_enabled = !Rails.env.production?`. Wire in `init_bundles` initializer |
| `lib/ssr/deno/rails/generators/ssr/deno/templates/ssr_deno.rb` | Add commented-out config option |

### Other

| File | Change |
|---|---|
| `sig/ssr/deno.rbs` | Add type signatures for new methods + `SsrSourceMapper` |
| `CHANGELOG.md` | Add entry |

## Registration strategy

In `load_bundle_in_worker`, after the bundle is evaluated:

```rust
if source_maps {
    let map_path = format!("{}.map", bundle_path);
    if let Ok(map_data) = std::fs::read(&map_path) {
        if let Ok(sm) = sourcemap::SourceMap::from_slice(&map_data) {
            SSR_SOURCE_MAPPER.write().register(&bundle_path, sm, &map_path);
        }
    }
}
```

`script_name` for lookup = the bundle path string (same as used in V8 errors).

## Resolution strategy (IIFE-aware)

In the render error path, after V8 returns an error:

```rust
// render.rs: on error from V8
let raw_msg = format!("{e}");
let resolved = if source_maps {
    SSR_SOURCE_MAPPER.read().resolve(&raw_msg)
} else {
    raw_msg
};
Err(SSRDenoError::Render(resolved))
```

`resolve()` parses the error message with a regex for V8 stack frames:

```
at (script_name):(\d+):(\d+)
```

For each match:
1. `bundle_line = v8_line - 1` (IIFE offset)
2. Look up `script_name` in registered maps
3. Convert `(bundle_line, v8_col)` to `(source_file, source_line, source_col)` via `sourcemap::SourceMap::lookup`
4. Replace the match in the error string

## Error handling

- `.map` file missing → silently skip
- `.map` file corrupt → silently skip
- Position not found in source map → leave original position (best-effort)
- No map registered for bundle → leave original position
- Best-effort, never blocks or throws

## Multi-isolate

`SSR_SOURCE_MAPPER` is a global `RwLock<SsrSourceMapper>`. Shared across all
worker threads. Registration happens during the `load_bundle` broadcast (all
workers receive the same message). Since source maps are the same regardless
of which isolate loads them, the global registry avoids duplication.

But: `SsrSourceMapper` is behind `RwLock`, and `resolve()` is called from a
worker thread. Since `resolve()` is read-only, `RwLock` allows concurrent reads.
`register()` requires write access, which blocks until all reads complete.

Global `SSR_SOURCE_MAPPER`:

```rust
use std::sync::RwLock;
use ssr_deno_core::source_mapper::SsrSourceMapper;

static SSR_SOURCE_MAPPER: Lazy<RwLock<SsrSourceMapper>> =
    Lazy::new(|| RwLock::new(SsrSourceMapper::new()));
```

## Tests

### Rust unit tests (ssr_deno_core)

- `SsrSourceMapper::resolve()` with known source map + known input string
- IIFE line offset correction (`v8_line - 1`)
- No map registered → returns original string unchanged
- Corrupt source map → returns original string unchanged

### Ruby integration tests

**Test fixture:** generate a bundle with a deliberate throw at a known line, plus a matching `.js.map` sidecar:

```ruby
# test/fixtures/throw-bundle.js
globalThis.render = function() {
  throw new Error('test-error');
};
```

```json
// test/fixtures/throw-bundle.js.map
{
  "version": 3,
  "file": "throw-bundle.js",
  "sources": ["components/thrower.tsx"],
  "sourcesContent": ["globalThis.render = function() {\n  throw new Error('test-error');\n};"],
  "names": [],
  "mappings": "AAAA;AACA"
}
```

The VLQ mappings in `"mappings": "AAAA;AACA"` decode to:

| Segment | Decoded |
|---------|---------|
| `AAAA` (line 0 gen) | gen_col=0, source=0, orig_line=0, orig_col=0 |
| `AACA` (line 1 gen) | gen_col=0, source=0, orig_line=1, orig_col=0 |

This maps bundle line 2 (`throw ...`) → `components/thrower.tsx:2:0`.

**Test 1 — source maps enabled, error resolves to original path:**

```ruby
def test_source_map_resolves_error_location
  Dir.mktmpdir do |dir|
    js_path = File.join(dir, 'throw-bundle.js')
    map_path = "#{js_path}.map"

    File.write(js_path, <<~JS)
      globalThis.render = function() {
        throw new Error('test-error');
      };
    JS

    File.write(map_path, <<~JSON)
      {
        "version": 3,
        "file": "throw-bundle.js",
        "sources": ["components/thrower.tsx"],
        "sourcesContent": ["globalThis.render = function() {\\n  throw new Error('test-error');\\n};"],
        "names": [],
        "mappings": "AAAA;AACA"
      }
    JSON

    bundle = SSR::Deno::Bundle.new(js_path)
    error = assert_raises(SSR::Deno::RenderError) do
      bundle.render({})
    end

    # With source maps enabled, the error message should reference
    # the original source file, not the minified bundle
    assert_includes error.message, 'components/thrower.tsx'
    refute_includes error.message, 'throw-bundle.js'
  end
end
```

**Test 2 — source maps disabled, raw V8 message preserved:**

Same setup but with `source_maps_enabled = false`:

```ruby
def test_source_map_disabled_preserves_raw_v8_message
  Dir.mktmpdir do |dir|
    # ... same bundle creation ...

    SSR::Deno.source_maps_enabled = false
    bundle = SSR::Deno::Bundle.new(js_path)
    error = assert_raises(SSR::Deno::RenderError) do
      bundle.render({})
    end

    assert_includes error.message, 'throw-bundle.js'
  end
end
```

**Test 3 — .map file missing, error unchanged:**

Tests the silent-skip error handling:

```ruby
def test_source_map_missing_does_not_raise
  Dir.mktmpdir do |dir|
    js_path = File.join(dir, 'throw-bundle.js')
    # No .map file created

    File.write(js_path, <<~JS)
      globalThis.render = function() {
        throw new Error('test-error');
      };
    JS

    SSR::Deno.source_maps_enabled = true
    bundle = SSR::Deno::Bundle.new(js_path)
    error = assert_raises(SSR::Deno::RenderError) { bundle.render({}) }

    assert_includes error.message, 'throw-bundle.js'
  end
end
```

All three tests use `assert_raises` (not subprocess) because the error is a
recoverable `RenderError` — the pool stays usable after catching it.

### Expected Ruby-level error display

**Before (source maps disabled):**

```
SSR::Deno::RenderError: Error: test-error
  at throw-bundle.js:2:9
```

**After (source maps enabled):**

```
SSR::Deno::RenderError: Error: test-error
  at components/thrower.tsx:2:9
```

## Steps

- [ ] ◐ **Plan written** — this file
- [ ] **ssr_deno_core: Cargo.toml** — add `sourcemap = "9"`
- [ ] **ssr_deno_core: source_mapper.rs** — `SsrSourceMapper` with `register`, `resolve`, `clear`
- [ ] **ssr_deno_core: lib.rs** — add `source_maps: bool` to `Config`, add `SsrSourceMapper` error variant
- [ ] **ssr_deno_core: tests** — unit test `resolve` with mock source map, IIFE offset
- [ ] **ext/ssr_deno: Cargo.toml** — add `sourcemap = "9"`, re-export `SsrSourceMapper`
- [ ] **ext/ssr_deno: types.rs + pool.rs + handle.rs** — thread `source_maps` flag
- [ ] **ext/ssr_deno: worker.rs** — register maps in `load_bundle_in_worker`
- [ ] **ext/ssr_deno: render.rs** — resolve error messages via `SSR_SOURCE_MAPPER`
- [ ] **ext/ssr_deno: lib.rs** — FFI for enabling/disabling, static init
- [ ] **Ruby: Config** — `config.rb` setters + env var
- [ ] **Ruby: Railtie** — `railtie.rb` default `!Rails.env.production?`
- [ ] **Ruby: Generator template** — commented-out option
- [ ] **RBS** — new type signatures
- [ ] **Ruby: tests** — integration tests for source map resolution:
  - `test_source_map_resolves_error_location`
  - `test_source_map_disabled_preserves_raw_v8_message`
  - `test_source_map_missing_does_not_raise`
- [ ] **Stale audit** — README, CHANGELOG, Cargo.toml comments, comments in source
- [ ] **`bundle exec rake`** — full pipeline passes
