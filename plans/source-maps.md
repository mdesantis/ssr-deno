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
  B -->|native_load_bundle| C[Pool.load_bundle]
  C -->|source_maps enabled?| D[Read .js.map in lib.rs]
  D -->|SsrSourceMapper.register| E[Global RwLock map registry]
  E -->|error occurs| F[native_render returns Err]
  F -->|map_render_error| G[SSR_SOURCE_MAPPER.resolve]
  G -->|lookup + IIFE offset| H[Rewritten error with .tsx path]
  H -->|Ruby RenderError| I["file.ts: line:col"]
```

**Key design decisions (deviations from initial plan):**

1. **Registration in FFI layer** (`lib.rs`), not in `load_bundle_in_worker` (`worker.rs`). The pool's broadcast completes first, then `native_load_bundle` registers the map on the Ruby thread. No message-type changes needed.
2. **Resolution in `map_render_error`** (`lib.rs`), not in `render.rs`. The error string is resolved before creating the Ruby exception class. No render-path changes needed.
3. **No changes to `types.rs`, `pool.rs`, `handle.rs`, `worker.rs`, or `render.rs`.** Source maps are entirely handled in the FFI layer + `SsrSourceMapper`.

## IIFE line offset

Bundles are wrapped in `(function(){\n...\n})();` at `worker.rs:213`. Two lines
are added before the bundle (`(function(){\n`), so V8 reports positions shifted
by +2 relative to the source map's generated positions:

```
V8 line 1 = "(function(){"          (IIFE prefix — not in bundle)
V8 line 2 = bundle line 1            = source map generated index 0
V8 line N = bundle line N-1          = source map generated index N-2
```

Resolution adjusts: `sourcemap_line = v8_line.saturating_sub(2)`.

## Self-managed source map registry — no deno_core patching

Avoids `build.rs` registry hacks. `SsrSourceMapper` lives in `ssr_deno_core`
(the pure-Rust crate with no V8 dep) so it's fast to compile and testable with
`cargo test -p ssr_deno_core`.

```rust
// crates/ssr_deno_core/src/source_mapper.rs
pub struct SsrSourceMapper {
    maps: HashMap<String, (sourcemap::SourceMap, SystemTime)>,
}

impl SsrSourceMapper {
    pub fn new() -> Self { ... }
    pub fn register(&mut self, bundle_path: &str, map_path: &Path) { ... }
    pub fn resolve(&self, msg: &str) -> String { ... }
    pub fn clear(&mut self) { ... }
}
```

`register` reads the `.map` file from disk and parses it. Skips if mtime
unchanged (caching). `resolve` line-by-line: matches V8 stack frame pattern
`at <file>:<line>:<col>` or `at func (<file>:<line>:<col>)`, adjusts IIFE
offset, looks up original source position, replaces in output.

### Sourcemap crate

Added only to `crates/ssr_deno_core/Cargo.toml` (pure-Rust crate, no V8 dep):

```toml
[dependencies]
sourcemap = "9"
```

### Registration flow

In `native_load_bundle` (lib.rs), after `pool.load_bundle` succeeds:

```rust
if lock_config().source_maps {
    let map_path = Path::new(&bundle_path).with_extension("js.map");
    get_source_mapper().write().register(&bundle_path, &map_path);
}
```

Registration runs on the Ruby thread (not in worker threads). The `RwLock`
write guard ensures no concurrent access with render error resolution.

## Files changed (actual)

### Rust layer — ssr_deno_core (pure Rust)

| File | Change |
|---|---|
| `crates/ssr_deno_core/Cargo.toml` | Added `sourcemap = "9"` dep |
| `crates/ssr_deno_core/src/lib.rs` | Added `pub mod source_mapper`, `source_maps: bool` to `Config` (default `false`) |
| `crates/ssr_deno_core/src/source_mapper.rs` | **New** — `SsrSourceMapper` with `register`, `resolve`, `clear` |

### Rust layer — ssr_deno (main crate)

| File | Change |
|---|---|
| `ext/ssr_deno/src/lib.rs` | Imported `SsrSourceMapper`. Added `get_source_mapper()` global via `OnceLock<RwLock<...>>`. Added `native_set_source_maps_enabled` / `native_get_source_maps_enabled` FFI. Registration in `native_load_bundle`. Resolution in `map_render_error`'s `Render` arm. |

No changes to `types.rs`, `pool.rs`, `handle.rs`, `worker.rs`, or `render.rs`.

### Ruby layer

| File | Change |
|---|---|
| `lib/ssr/deno/config.rb` | Added `source_maps_enabled=` setter, `source_maps_enabled?` getter, `SSR_DENO_SOURCE_MAPS_ENABLED` env var |
| `lib/ssr/deno/rails/railtie.rb` | Default `config.ssr_deno.source_maps_enabled = !Rails.env.production?`, wired in `init_bundles` |
| `lib/ssr/deno/rails/generators/ssr/deno/templates/ssr_deno.rb` | Added commented-out config option |

### Other

| File | Change |
|---|---|
| `sig/ssr/deno.rbs` | Added `source_maps_enabled=` / `source_maps_enabled?` signatures, native FFI signatures |
| `CHANGELOG.md` | Added entry under Unreleased |
| `README.md` | Runtime settings code block, env vars table, Source maps subsection, Rails config list |
| `docs/architecture.md` | Config getters list, Rust layer table |

## Error handling

- `.map` file missing → silently skip (register does nothing)
- `.map` file corrupt → silently skip (`from_slice` returns Err)
- Position not found in source map → leave original position (best-effort)
- No map registered for bundle → original string unchanged via `resolve`
- Best-effort, never blocks or throws

## Multi-isolate

`SSR_SOURCE_MAPPER` is a global `RwLock<SsrSourceMapper>` behind a
`OnceLock`. Resolution (`read`) is concurrent. Registration (`write`) is
exclusive — runs once per `native_load_bundle` call (Ruby thread, not in the
render hot path).

```rust
fn get_source_mapper() -> &'static RwLock<SsrSourceMapper> {
    static MAPPER: OnceLock<RwLock<SsrSourceMapper>> = OnceLock::new();
    MAPPER.get_or_init(|| RwLock::new(SsrSourceMapper::new()))
}
```

## Tests

### Rust unit tests (ssr_deno_core) — implemented, 41/41 pass

- `resolve_no_map_returns_original` — no maps registered, unchanged
- `resolve_empty_message` — empty input returns empty
- `resolve_non_frame_line_left_alone` — non-stack-frame lines unchanged
- `resolve_iife_offset_corrected` — V8 line 3 → bundle line 1, resolves to .tsx
- `resolve_with_func_name` — parens format `at func (file:line:col)`
- `resolve_map_line_beyond_map_uses_closest_token` — line beyond map gets closest match
- `resolve_unregistered_bundle_left_alone` — different bundle name, unchanged
- `register_skips_unchanged_map` — mtime caching
- `register_missing_map_does_nothing` — missing .map, no panic
- `clear_removes_all_maps` — clear works
- Config defaults test for `source_maps`

### Ruby integration tests — planned, not yet written

Run when `bundle exec rake test` from a build-capable environment.

- `test_source_map_resolves_error_location` — verify `.tsx` path in error
- `test_source_map_disabled_preserves_raw_v8_message` — verify `.js` path preserved
- `test_source_map_missing_does_not_raise` — no `.map`, no crash

## Expected Ruby-level error display

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

- [x] **Plan written** — this file
- [x] **ssr_deno_core: Cargo.toml** — added `sourcemap = "9"`
- [x] **ssr_deno_core: source_mapper.rs** — `SsrSourceMapper` with `register`, `resolve`, `clear`
- [x] **ssr_deno_core: lib.rs** — added `source_maps: bool` to `Config`, `pub mod source_mapper`
- [x] **ssr_deno_core: tests** — 10 unit tests, all passing
- [x] **ext/ssr_deno: lib.rs** — global `get_source_mapper()`, FFI, registration in `native_load_bundle`, resolution in `map_render_error`
- [x] **Ruby: Config** — `config.rb` setters + env var
- [x] **Ruby: Railtie** — `railtie.rb` default `!Rails.env.production?`
- [x] **Ruby: Generator template** — commented-out option
- [x] **RBS** — type signatures for new methods
- [x] **CHANGELOG** — Unreleased entry
- [x] **README + docs** — Runtime settings, env vars table, Source maps subsection, architecture.md
- [x] **Ruby: integration tests** — `test_source_map_resolves_error_location`, `test_source_map_disabled_preserves_raw_v8_message`, `test_source_map_missing_does_not_crash` — all pass
- [x] **`bundle exec rake`** — full pipeline passes: compile, cargo test (41/41), cargo clippy (clean), cargo fmt (clean), Vite samples build, Ruby tests (all suites, 0 failures), RuboCop (0 offenses), RBS validation, coverage 100%/100%
