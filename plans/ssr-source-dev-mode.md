# SSR Source Dev Mode — skip the build step entirely

## Problem

Developer currently runs 3 processes (`web` + `vite` + `rolldown --watch`) plus an npx-rolldown pre-build. SSR requires a bundling step before Rails boots. This leaks build infrastructure into the dev workflow.

## Goal

A Rails dev can run `bin/rails s` and SSR just works. No Procfile entry, no overmind, no npx, no pre-build. The gem loads source `.tsx` files directly into the embedded Deno V8 runtime. Only the client-side Vite dev server remains external.

## Design constraint — separate code paths

Production path is **untouched**. No `if dev ... else ...` branching in production functions. Dev mode is implemented as separate modules with their own entry points.

## Architecture

### Production (unchanged)

```
pre-built .js file → [Rust: read, IIFE, execute_script] → globalThis.render in V8
                      ↳ NoopModuleLoader
                      ↳ Permissions::none_without_prompt()
                      ↳ load_bundle(), render(), render_chunked()
```

### Dev mode (new)

```
source .tsx entry → [Ruby: optional codegen of __ssr_imports__.ts (only if import.meta.glob present)]
                  → [Rust: DevModuleLoader resolves @/, node_modules, reads files]
                  → [Rust: deno_ast transpile TS/JSX → JS + inline source map registered]
                  → [Rust: mod_evaluate() → globalThis.render in V8]
                   → render via shared engine functions, dispatched through dev-worker FFI
```

The render engine **functions** (`render::render()` / `render_chunked::render_chunked()`, both accept `&mut MainWorker`) are reused. The V8 isolate pool is **not** shared — dev mode uses dedicated single-isolate workers outside the pool. Pool stays `'static` + private behind `OnceLock`; dev workers are owned by Ruby `DevBundle` instances via opaque handle.

**Render dispatch:** `native_render` → `IsolatePool::dispatch_render` → `self.handles[idx]`. Pool handles vec private + static (`POOL: OnceLock<IsolatePool>`). Dev worker unreachable through existing FFI. **Mandatory new FFI surface**:
- `native_dev_render(handle, bundle_id, args_json)`
- `native_dev_render_chunks(handle, bundle_id, args_json, &block)`
- `native_dev_load_entry(handle, entry_path, alias_map_json)`
- `native_dev_worker_new(project_root, max_heap_size_mb, render_timeout_ms) -> handle`

DevBundle holds opaque `usize`/`magnus::TypedData` handle to a single `IsolateHandle`-shaped struct (1 isolate, same channel pattern as pool). Bundle lookup itself is JS-side per isolate (`globalThis.__ssr_bundles[id]`) — each dev worker has independent globals.

### Pool isolation

| Aspect | Production pool | Dev workers |
|--------|----------------|-------------|
| **Loader** | `NoopModuleLoader` | `DevModuleLoader` |
| **Concurrency** | Multi-isolate (default 1, configurable) | Single-isolate per DevBundle |
| **Permissions** | `Permissions::none_without_prompt()` | Read-only for project root |
| **Worker count** | `Config::isolate_pool_size` | 1 per `DevBundle` instance |
| **Builder** | `build_worker()` | `build_dev_worker()` — separate function, same `MainWorker::bootstrap_from_options` |

`build_dev_worker()` mirrors `build_worker()` but:
- Passes `DevModuleLoader` instead of `NoopModuleLoader` / `NodeBuiltinOnlyModuleLoader`
- Grants `--allow-read` for the project directory (`PermissionsOptions { allow_read: Some(vec![project_root.into_owned()]), .. }`)
- Keeps all other restrictions (no net, no env, no run, no write, no ffi, no sys)
- **Re-registers `add_near_heap_limit_callback`** (parity with prod `builder.rs:163` — else dev OOM crashes process)
- **Re-registers Web Workers panic guard** (`create_web_worker_cb` from `builder.rs:135`)
- **Always enables `deno_node` extension** (irrespective of `Config.node_builtins`) — required for `node_modules` resolution and `node:*` polyfills. Production `node_builtins` flag is ignored in dev path.
- **Always invokes `worker::setup_require`** after entry load — `globalThis.require` available for CJS-only packages (MUI internals, etc.). Cost ~10ms once, idempotent.

### Permissions for dev mode

`Permissions::none_without_prompt()` denies file reads. `DevModuleLoader` must read source `.tsx` files from disk. Dev workers use a relaxed permission set restricted to the project root.

**API shape verified** (spike against `deno_permissions 0.106.0`, pinned by `deno_runtime 0.255.0`):

```rust
// file: /home/maurizio/.cargo/registry/src/.../deno_permissions-0.106.0/lib.rs:3591
// PermissionsOptions has #[derive(Default)]
use deno_runtime::deno_permissions::{
    Permissions, PermissionsOptions, RuntimePermissionDescriptorParser,
};

let opts = PermissionsOptions {
    allow_read: Some(vec![project_root.to_string_lossy().into_owned()]),
    prompt: false,
    ..Default::default()
};
let perms = Permissions::from_options(
    &RuntimePermissionDescriptorParser::new(Sys),
    &opts,
).map_err(|e| format!("Permissions::from_options: {e}"))?;
```

Key correction from previous draft: `from_options` takes `&dyn PermissionDescriptorParser` (not a sys), returns `Result<Self, PermissionsFromOptionsError>` (not bare `Self`). `Sys` satisfies all trait bounds via blanket impls. `RuntimePermissionDescriptorParser<Sys>` is used as the parser.

Only the project root is readable. `node_modules/` under the project root is accessible. Everything else is denied.

### Module loading performance

Deno's `ModuleLoader` resolves and loads one module per `import` statement. MUI's dependency graph is deep (~500+ modules for a full tree). Each load = resolve specifier + read file + `deno_ast` parse/transpile. First-load latency could be **seconds**, not milliseconds.

Mitigations:
1. **Transpiled module cache** in `DevModuleLoader` — cache by file mtime across reloads (per-file mtime; also drives auto-reload — single source of truth)
2. **Single-threaded** — dev doesn't need multi-worker, one worker is acceptable

**Out of scope (v2):** `v8_code_cache` option (field on `WorkerServiceOptions`, currently `None` in `builder.rs:102`) — requires `Arc<dyn CodeCache>` impl. Defer.

If first-load is too slow (>2-3s), add a **warmup step** in `DevBundle#initialize` that pre-loads the entry before the first render is requested.

This is a one-time cost. **HMR** (future, see below) would mitigate this fully — after the initial load, only changed modules are re-transpiled and swapped in the V8 module map. The remaining ~499 cached modules are untouched.

### Emotion / CSS-in-JS

Emotion's SSR path (`renderToString` + `extractCriticalToChunks` + `constructStyleTagsFromChunks`) is pure runtime — it emits HTML `<style>` tags during render, not at build time. Works identically whether loaded via Rolldown bundle or dev mode's direct module evaluation.

The `isBrowser()` guard in `createEmotionCache` already handles the Deno V8 context (no DOM → skip insertion point logic, no `window.__CSP_NONCE__`). Nothing to change.

### CSS and non-JS imports

Some component files import CSS or other non-JS assets. The `DevModuleLoader` handles these by:
- `.css` → return empty module (no-op)
- `.svg`, `.png`, etc. → return empty module
- Unknown extensions → return empty module with a dev-mode debug warning

Exact list of ignored extensions matches what `ssr.noExternal: true` in Vite config effectively does.

### Auto-reload strategy

`DevModuleLoader` maintains a per-file mtime cache (entry: `{path, mtime, transpiled_js, source_map}`). On every render, `DevBundle#reload_if_changed` calls `native_dev_check_stale(handle)` which walks the cache, stat-s each tracked path, and returns `true` if any current mtime exceeds the cached mtime.

On stale → **drop + respawn worker** (v1):
1. Drop the dev worker's `Sender<WorkerMsg>` → channel closes → worker thread exits cleanly → `MainWorker` dropped → V8 isolate torn down
2. `DevBundle` calls `native_dev_worker_new` to spawn a fresh worker
3. `native_dev_load_entry` re-evaluates the entry; transpile cache rebuilt from scratch (no carry-over to avoid stale V8 module refs)

Trade-off accepted: full re-init (~1-3s on a 500-module graph) on every change. V8 state guaranteed clean — no leaked modules, no zombie globals. In-flight renders during reload fail with `JsRuntimeWorkerError` — acceptable in Rails dev (serial-ish request stream).

Hot module replacement (preserve worker, swap modules in V8 module map) is **v2**, listed under [Future](#future).

### Source maps for transpiled code

`deno_ast` emits inline source maps by default (`SourceMapOption::Inline`). The source map is embedded as a `//# sourceMappingURL=data:application/json;base64,...` comment at the end of the emitted JS. Alternatively, `SourceMapOption::Separate` returns the map as a separate string in `EmittedSourceText.source_map`.

Verified API against `deno_ast 0.53.1` (`src/emit.rs:14`, `src/transpiling/mod.rs:278`):

```rust
let result = parsed.transpile(
    &TranspileOptions { jsx: Some(...), ..Default::default() },
    &TranspileModuleOptions::default(),
    &EmitOptions {
        source_map: SourceMapOption::Separate,  // or Inline (default)
        ..Default::default()
    },
)?.into_source();
// result.text -> JS output (+ inline sourcemap comment if Inline)
// result.source_map -> Some(json_string) when Separate, None when Inline
```

`DevModuleLoader::load` registers the map with the existing `SsrSourceMapper` keyed by the absolute file path (not the bundle path used in prod).

`SsrSourceMapper::register` currently reads `.js.map` from disk ([source_mapper.rs:29](ext/ssr_deno/crates/ssr_deno_core/src/source_mapper.rs#L29)). Add `register_inline(path, sourcemap_bytes, mtime)` variant that skips the file read. Error stack frames in dev resolve to `.tsx` originals — DX parity with prod source-map flow.

Without this, V8 stack frames point at transpiled JS — unreadable. Mandatory for dev to be usable.

### Codegen lifecycle

`__ssr_imports__.ts` only needed when the entry uses `import.meta.glob` (Vite-only API, no Deno equivalent). Inspect the side-project entry first — if no `import.meta.glob`, **skip codegen entirely**: `DevBundle.new(entry_path)` is enough.

If codegen needed:
- Ruby regex strips `import.meta.glob(...)` calls; replaces with static `import { X } from '...'` lines built from `Dir.glob`
- Auto-regen triggered by per-file mtime change tracked in `DevModuleLoader`'s transpile cache (single source of truth; no separate dir-mtime path)

### Worker builder

`build_dev_worker()` is a separate function, separate file:

**File:** `ext/ssr_deno/src/deno_runtime_wrapper/dev_builder.rs`

```rust
pub fn build_dev_worker(
    main_module: &Url,
    max_heap_size_mb: usize,
    resolve_aliases: HashMap<String, PathBuf>,
    project_root: &Path,
) -> Result<MainWorker, String> {
    // 1. ByonmNpmResolver<Sys> + ByonmInNpmPackageChecker (from deno_resolver::npm)
    // 2. DevModuleLoader with aliases (delegates npm resolution to (1))
    // 3. Permissions::from_options with allow_read=[project_root]
    // 4. Same V8 create_params, extensions, near-heap-limit cb, web-worker guard
}
```

## Existing resolver infrastructure

Concrete npm-resolver impls already shipped by `deno_resolver 0.78.0` (transitive dep through `deno_runtime 0.255.0`, see Cargo.lock). No walker needed.

| Crate / type | Status |
|--------------|--------|
| `node_resolver = "=0.85.0"` | Direct dep, [Cargo.toml:28](ext/ssr_deno/Cargo.toml#L28) |
| `NodeResolver` (in `deno_node`) | Already used in [builder.rs:42](ext/ssr_deno/src/deno_runtime_wrapper/builder.rs#L42) |
| `PackageJsonResolver` | Already used in [builder.rs:41](ext/ssr_deno/src/deno_runtime_wrapper/builder.rs#L41) |
| `DenoIsBuiltInNodeModuleChecker` | Already used in [builder.rs:45](ext/ssr_deno/src/deno_runtime_wrapper/builder.rs#L45) |
| `deno_resolver::npm::ByonmNpmResolver<Sys>` | **New direct dep needed.** Implements `NpmPackageFolderResolver` for host-managed `node_modules/` (`byonm.rs:71`, `NpmPackageFolderResolver` impl at line 327 of `npm/mod.rs` covers all `NpmResolver` variants — Byonm is the right variant for our use case). |
| `deno_resolver::npm::ByonmInNpmPackageChecker` | **New direct dep needed.** Concrete `InNpmPackageChecker` for BYONM (`byonm.rs:501-503`). |

**BYONM** ("Bring Your Own node_modules") is the right primitive: user runs `npm install` / `pnpm install` / `yarn install` independently. `ByonmNpmResolver` walks the result; symlinked layouts (pnpm's `.pnpm` store) handled by the implementation — no special-casing required.

Production suppresses npm resolution via `NopInNpmPackageChecker` + `NopNpmPackageFolderResolver`. Dev mode swaps in `ByonmInNpmPackageChecker` + `ByonmNpmResolver<Sys>`. `Sys` already satisfies `ByonmNpmResolverSys` via existing `FsRead + FsMetadata` impls in [sys.rs](ext/ssr_deno/src/sys.rs).

## New Rust module: `dev_module_loader`

**File:** `ext/ssr_deno/src/dev_module_loader.rs`

Implements `deno_core::ModuleLoader`:

| Specifier type | Resolution |
|----------------|------------|
| `@/foo/bar` | Intercepted at ModuleLoader level → `app/frontend/foo/bar.ts` (ts → tsx → js → jsx extension fallback) |
| `./relative` | Relative to parent module, same extension fallback |
| Bare `foo` / `@scope/foo` | `NodeResolver<ByonmInNpmPackageChecker, ByonmNpmResolver<Sys>, Sys>` walks user-managed `node_modules/` |
| `npm:foo@1.2` URL | Same path as bare — strip `npm:` prefix; mostly unused if user code came from Vite/Rolldown |
| `node:*` | Served by `deno_node` extension (not loader). `DevModuleLoader::resolve` only needs `ModuleSpecifier::parse(specifier)` for `node:` scheme (same pattern as [`NodeBuiltinOnlyModuleLoader::resolve`](ext/ssr_deno/src/node_builtin_loader.rs#L23-L24)); `load()` never called for `node:*` polyfills |
| `.css`, `.svg`, `.png`, … | Empty module (no-op) |
| `import.meta.glob` | Stripped by Ruby codegen before entry hits Rust (regex-replace) |

Uses Deno's native `deno_ast` for transpilation (TS strip + JSX → JS). `deno_ast` is already compiled in — `deno_runtime/hmr` (always on) pulls in `deno_runtime/transpile` which pulls in `deno_ast 0.53.1` with `transpiling` feature (emit + proposal + react + transforms + typescript). No new dependency cost. The `#[cfg(feature = "dev-mode")]` flag gates the Rust code paths only.

## New Rust module: `dev_builder`

**File:** `ext/ssr_deno/src/deno_runtime_wrapper/dev_builder.rs`

Separate `build_dev_worker()` function, no `if dev` branching in the production `build_worker()`. Accepts project root for permissions and alias map for the module loader.

## New Rust module: `dev_load`

**File:** `ext/ssr_deno/src/deno_runtime_wrapper/dev_load.rs`

```rust
pub fn dev_load_entry(
    worker: &mut MainWorker,
    entry_path: &Path,
    resolve_alias: &HashMap<String, String>,
) -> Result<(), String> {
    // 1. Resolve entry to absolute path
    // 2. Create DevModuleLoader with aliases
    // 3. Tell worker to evaluate entry as ES module
    // 4. After eval, move globalThis.render → __ssr_bundles[id]
}
```

Does NOT wrap in IIFE. Uses Deno's ES module evaluation chain instead.

## New Ruby class: `SSR::Deno::DevBundle`

Parallel to `Bundle`, same `#render` / `#render_chunks` interface — registers in `Bundle.registry` for `find_bundle!` polymorphism.

```ruby
class DevBundle
  def initialize(entry_path, resolve_alias: { '@' => 'app/frontend' }, project_root: Dir.pwd)
    @entry_path = entry_path.to_s
    @resolve_alias = resolve_alias
    @project_root = project_root.to_s
    @handle = SSR::Deno.native_dev_worker_new(@project_root,
                                              Config.max_heap_size_mb,
                                              Config.render_timeout_ms)
    regenerate_imports! if entry_uses_import_meta_glob?
    SSR::Deno.native_dev_load_entry(@handle, @entry_path, @resolve_alias)
  end

  def render(data = nil, raw_input: false, raw_output: false)
    reload_if_changed
    json = raw_input ? data : JSON.generate(data)
    result = SSR::Deno.native_dev_render(@handle, @entry_path, json)
    raw_output ? result : JSON.parse(result)
  end

  def render_chunks(data = nil, raw_input: false, &block)
    reload_if_changed
    json = raw_input ? data : JSON.generate(data)
    SSR::Deno.native_dev_render_chunks(@handle, @entry_path, json, &block)
  end

  private

  def reload_if_changed
    return unless SSR::Deno.native_dev_check_stale(@handle)
    regenerate_imports! if entry_uses_import_meta_glob?
    SSR::Deno.native_dev_load_entry(@handle, @entry_path, @resolve_alias)
  end

  def regenerate_imports!
    # Regex-strip import.meta.glob, replace with Dir.glob-built static imports.
    # Pure Ruby — no Node.js, no tsx.
  end

  def entry_uses_import_meta_glob?
    File.read(@entry_path).include?('import.meta.glob')
  rescue Errno::ENOENT
    false
  end
end
```

## Cargo changes

Add optional `dev-mode` feature flag. This gates the Rust code (module loader, builder), **not** any dependency — `deno_ast` is already compiled via `deno_runtime/hmr`.

```toml
[features]
default = []
dev-mode = []  # gates DevModuleLoader + dev_builder Rust code

[dependencies]
# deno_ast already available via deno_runtime/hmr → transpile → deno_ast
deno_runtime = { version = "0.255.0", features = ["hmr"] }
```

**No new crate added.** The `node_resolver` crate is already at `=0.85.0`. The `NpmPackageFolderResolver` and `InNpmPackageChecker` traits are already in the dependency tree — dev mode provides real implementations instead of NOPs.

## New Ruby API

```ruby
# config/initializers/ssr_deno.rb
SSR::Deno.configure do |c|
  c.dev do |d|
    d.entry :app, Rails.root.join('app/frontend/entrypoints/ssr-app.tsx')
    d.entry :demos, Rails.root.join('app/frontend/entrypoints/ssr-demos.tsx')
    d.alias '@', 'app/frontend'
  end
end
```

Each `d.entry` builds a `DevBundle` and inserts it into the shared `SSR::Deno::Bundle.registry`. `SSR::Deno::Helpers.find_bundle!(:app)` returns the DevBundle transparently — same interface as `Bundle`.

Or skip the DSL and create one directly:

```ruby
SSR::Deno::DevBundle.new(
  Rails.root.join('app/frontend/entrypoints/ssr-app.tsx'),
  resolve_alias: { '@' => 'app/frontend' }
)
```

## Dependency graph (dev path only)

```
Ruby: DevBundle.new(entry.tsx)
  → Ruby: (optional) strip import.meta.glob, write __ssr_imports__.ts
  → Rust: native_dev_worker_new(project_root, heap_mb, timeout_ms) -> handle
    → spawns dev worker thread, calls build_dev_worker (DevModuleLoader + relaxed perms)
  → Rust: native_dev_load_entry(handle, entry_path, alias_map)
    → DevModuleLoader resolves graph (@/, ./, bare, node:*, .css→noop)
    → deno_ast transpiles each .ts/.tsx, registers inline sourcemap in SsrSourceMapper
    → ES module evaluated, globalThis.__ssr_bundles[entry_path] = { render }
  → Ruby: DevBundle stored in Bundle.registry (polymorphic with Bundle)
  → Ruby: bundle.render(data)
    → native_dev_render(handle, entry_path, args_json)
    → dev worker dispatches via shared render::render(&mut MainWorker, ...)
    → JS error → stack frames mapped via SsrSourceMapper to .tsx originals
```

## What does NOT change

- `render::render()` / `render_chunked::render_chunked()` *engine* functions — untouched, reused with dev `&mut MainWorker`
- `Bundle` Ruby class — untouched
- `Config` (Ractor-safe singleton) — dev mode owns its own state via DevBundle
- `NoopModuleLoader` / `NodeBuiltinOnlyModuleLoader` — stay for production
- `NopInNpmPackageChecker` / `NopNpmPackageFolderResolver` — stay for production
- `build_worker()` — untouched, production only
- V8 isolate pool (`IsolatePool`, `POOL` `OnceLock`) — untouched, dev mode uses separate single-isolate workers
- `Permissions::none_without_prompt()` — stays for production
- `Cargo.toml` default features — `dev-mode` gates Rust code only (no dependency change)
- Existing FFI surface (`native_render`, `native_render_chunks`, etc.) — untouched
- `SsrSourceMapper` core — extended with `register_inline()`, existing `register()` from disk unchanged
- Test suite — existing tests all test production path, continue passing

## What changes

| Component | Change |
|-----------|--------|
| `ext/ssr_deno/Cargo.toml` | Add `[features]` with optional `dev-mode` flag |
| `ext/ssr_deno/src/dev_module_loader.rs` | **New** — ModuleLoader impl for dev (alias resolution, npm resolution, node:* delegation, CSS/asset no-ops, transpile + inline source map, per-file mtime cache) |
| `ext/ssr_deno/src/deno_runtime_wrapper/dev_builder.rs` | **New** — `build_dev_worker()` separate from prod builder; includes near-heap-limit cb + web-worker panic guard parity |
| `ext/ssr_deno/src/deno_runtime_wrapper/dev_handle.rs` | **New** — `DevIsolateHandle` (single-isolate variant of `IsolateHandle`, owns a `Sender<WorkerMsg>`) |
| `ext/ssr_deno/src/deno_runtime_wrapper/dev_worker.rs` | **New** — dev worker thread main (mirrors `worker::worker_thread_main`, calls `build_dev_worker`) |
| `ext/ssr_deno/src/deno_runtime_wrapper/dev_load.rs` | **New** — ES module evaluation of entry → `globalThis.__ssr_bundles[id]` |
| `ext/ssr_deno/src/lib.rs` | Add `#[magnus::function]` entries: `native_dev_worker_new`, `native_dev_load_entry`, `native_dev_render`, `native_dev_render_chunks` |
| `ext/ssr_deno/Cargo.toml` | Add direct dep `deno_resolver = "=0.78.0"` |
| `ext/ssr_deno/src/real_npm_types.rs` | **New, thin** — re-export `ByonmNpmResolver<Sys>` + `ByonmInNpmPackageChecker` from `deno_resolver::npm::*`, plus a constructor `build_dev_npm_resolver(project_root) -> (ByonmInNpmPackageChecker, MaybeArc<ByonmNpmResolver<Sys>>)`. ~30 LOC, not a walker. |
| `ext/ssr_deno/crates/ssr_deno_core/src/source_mapper.rs` | Add `register_inline(path, sourcemap_bytes, mtime)` |
| `lib/ssr/deno.rb` | Expose `DevBundle` class |
| `lib/ssr/deno/dev_bundle.rb` | **New** — Ruby DevBundle class (holds dev-worker handle; registers in `Bundle.registry` for `find_bundle!` parity) |
| `lib/ssr/deno/dev_bundle/codegen.rb` | **New, optional** — Ruby-side `import.meta.glob` regex stripper. Skip if entry doesn't use it. |
| `sig/ssr/deno.rbs` | Add `DevBundle` signatures |

## Compile time risk

**No risk.** `deno_ast` (with `transpiling` feature) is already compiled in via `deno_runtime/hmr` → `deno_runtime/transpile` → `deno_ast`. The cost is already paid in every build. The `dev-mode` feature flag gates Rust code only — negligible compared to `deno_ast` itself.

## Known gaps

### Spike findings (resolved)

Spike against `~/.cargo/registry/src/.../{deno_permissions-0.106.0, deno_resolver-0.78.0, deno_ast-0.53.1}` complete; findings folded into the body sections above (Permissions, Existing resolver infrastructure, DevModuleLoader specifier table, Source maps). Summary citations retained for reviewers:

- `Permissions::from_options` — `deno_permissions-0.106.0/lib.rs:3591`
- `PermissionsOptions` `#[derive(Default)]` — lib.rs:3500
- `ByonmNpmResolver<Sys>` — `deno_resolver-0.78.0/npm/byonm.rs:71`
- `ByonmInNpmPackageChecker` — `byonm.rs:501`
- `TranspileResult::into_source` — `deno_ast-0.53.1/src/transpiling/mod.rs:62`
- `EmittedSourceText { text, source_map }` — `emit.rs:55`
- `SourceMapOption::Inline` `#[default]` — `emit.rs:14`
- `TranspileOptions::default { jsx: Some(_) }` — `transpiling/mod.rs:216`

### LOW — Open documentation items

These are decided behaviors that need a one-line callout in user-facing docs (README / `lib/ssr/deno/dev_bundle.rb` docstring):

- `SSR::Deno::Config.isolate_pool_size` has no effect on `DevBundle` (dev hardcodes 1 isolate per bundle).
- `SSR::Deno::Config.node_builtins` has no effect on `DevBundle` (always-on; required for `node_modules` resolution).
- DevBundle registers itself in `Bundle.registry` (polymorphic with `Bundle` — `Helpers.find_bundle!` works transparently).
- Auto-reload is full worker respawn — in-flight renders during reload fail with `JsRuntimeWorkerError`; acceptable in dev.
- CJS packages supported via auto-injected `globalThis.require`. ESM is preferred path.

## Implementation order

0. ~~**Spike**~~ ✅ DONE — all four targets verified, plan updated with confirmed API shapes.
1. ~~Add `dev-mode` feature flag to `Cargo.toml`~~ ✅ DONE — compiles clean
2. ~~Render-routing FFI stubs in `lib.rs`~~ ✅ DONE — 4 stub functions, cfg-gated, compiles clean
3. `dev_handle.rs` + `dev_worker.rs` — single-isolate worker mirroring `IsolateHandle`/`worker_thread_main`, calls `build_dev_worker`
4. `dev_builder.rs` — `build_dev_worker()` with parity to prod (heap-limit cb, web-worker panic guard, OOM atomic), real resolver(s) + dev permissions
5. `real_npm_types.rs` — re-export + tiny constructor wiring `ByonmNpmResolver<Sys>` + `ByonmInNpmPackageChecker` (no walker)
6. `dev_module_loader.rs` — alias resolution, npm/`node:` delegation, CSS/asset no-ops, transpile + inline source map, per-file mtime cache
7. `source_mapper.rs` — `register_inline()` API; wire dev module loads through it
8. `dev_load.rs` — entry evaluation, `globalThis.__ssr_bundles[id]` registration
9. Replace FFI stubs from step 2 with real logic
10. Ruby `DevBundle` (registers in `Bundle.registry`); optional `codegen.rb` only if entry uses `import.meta.glob`
11. Auto-reload: `native_dev_check_stale` queries module-cache; on `true` rebuild via fresh `dev_load_entry`
12. Test with side-project: remove Rolldown from Procfile, verify `rails s` boots SSR clean; verify source-map stack frames resolve to `.tsx` files
13. Update `plans/` index, ONBOARDING/README dev-mode section

## Future

- **`v8_code_cache`** wired with `Arc<dyn CodeCache>` impl to amortize first-load transpile cost across restarts
- **Hot module replacement**: swap individual modules in V8 module map without full worker rebuild
- **Oxc minifier** for dev bundle compression (closer-to-prod simulation)
- **Error overlay** in Rails for TS/JSX parse errors (intercept `deno_ast` parse failures, render BetterErrors-style page)
