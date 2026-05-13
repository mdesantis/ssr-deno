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
source .tsx entry → [Ruby: generate __ssr_imports__.ts via Dir.glob]
                  → [Rust: DevModuleLoader resolves @/, node_modules, reads files]
                  → [Rust: deno_ast transpile TS/JSX → JS]
                  → [Rust: mod_evaluate() → globalThis.render in V8]
                   → render via existing render() path (⚠️ see render routing gap below)
```

The render engine (`render()` / `render_chunked()`) is shared. The V8 isolate pool is **not** shared — dev mode uses dedicated single-isolate workers outside the pool. This keeps the pool's `NoopModuleLoader` untouched.

**⚠️ Render routing gap:** Dev workers live outside the pool, but `native_render` looks up bundle IDs in the pool's registered render functions. It won't find a dev worker. Need either a separate `native_dev_render` FFI or an alternative dispatch path. See [Known gaps](#known-gaps).

### Pool isolation

| Aspect | Production pool | Dev workers |
|--------|----------------|-------------|
| **Loader** | `NoopModuleLoader` | `DevModuleLoader` |
| **Concurrency** | Multi-isolate (default 1, configurable) | Single-isolate per DevBundle |
| **Permissions** | `Permissions::none_without_prompt()` | Read-only for project root |
| **Worker count** | `Config::isolate_pool_size` | 1 per `DevBundle` instance |
| **Builder** | `build_worker()` | `build_dev_worker()` — separate function, same `MainWorker::bootstrap_from_options` |

`build_dev_worker()` mirrors `build_worker()` but:
- Passes `DevModuleLoader` instead of `NoopModuleLoader`
- Grants `--allow-read` for the project directory via `Permissions::allow_read()`
- Keeps all other restrictions (no net, no env, no run)

### Permissions for dev mode

`Permissions::none_without_prompt()` denies file reads. `DevModuleLoader` must read source `.tsx` files from disk. Dev workers use a relaxed permission set:

```rust
Permissions {
    read: Some(AllowPermissionSet::allow_one(&project_root)),
    write: Some(AllowPermissionSet::deny_all()),
    net: Some(AllowPermissionSet::deny_all()),
    env: Some(AllowPermissionSet::deny_all()),
    run: Some(AllowPermissionSet::deny_all()),
    sys: Some(AllowPermissionSet::deny_all()),
    ffi: Some(AllowPermissionSet::deny_all()),
}
```

Only the project root is readable. `node_modules/` under the project root is accessible. Everything else is denied.

### Module loading performance

Deno's `ModuleLoader` resolves and loads one module per `import` statement. MUI's dependency graph is deep (~500+ modules for a full tree). Each load = resolve specifier + read file + `deno_ast` parse/transpile. First-load latency could be **seconds**, not milliseconds.

Mitigations:
1. **Transpiled module cache** in `DevModuleLoader` — cache by file mtime across reloads
2. **Deno's V8 code cache** — `v8_code_cache` option in `WorkerOptions` can cache compiled bytecode
3. **Single-threaded** — dev doesn't need multi-worker, so one worker is acceptable

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

### Codegen lifecycle

`__ssr_imports__.ts` is generated by Ruby `Dir.glob` before the Rust entry is loaded. If new component files are added during development, the imports file is stale. Strategies:

1. **Manual** — User restarts `rails s` (already needed for routing/config changes)
2. **mtime-based** — `reload_if_changed` checks if the `components/` directory mtime has changed, regenerates
3. **Controller action** — `GET /__ssr_deno/regen` triggers regeneration + reload

Strategy 2 is the default. Strategy 3 can be added as a dev helper.

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
    // 1. Real NpmPackageFolderResolver
    // 2. Real InNpmPackageChecker
    // 3. DevModuleLoader with aliases
    // 4. Permissions::allow_read(project_root)
    // 5. Same V8 create_params, extensions, etc.
}
```

## Existing resolver infrastructure

Deno already provides everything — no new resolver crate needed:

| Crate | Status in Cargo.toml |
|-------|---------------------|
| `node_resolver = "=0.85.0"` | Already present (line 28) |
| `NodeResolver` (in `deno_node`) | Already used in `builder.rs:42` |
| `PackageJsonResolver` | Already used in `builder.rs:41` |
| `DenoIsBuiltInNodeModuleChecker` | Already used in `builder.rs:45` |

Production suppresses npm resolution via `NopInNpmPackageChecker` + `NopNpmPackageFolderResolver`. Dev mode swaps in real implementations that read `node_modules/` from disk.

## New Rust module: `dev_module_loader`

**File:** `ext/ssr_deno/src/dev_module_loader.rs`

Implements `deno_core::ModuleLoader`:

| Specifier type | Resolution |
|----------------|------------|
| `@/foo/bar` | Intercepted at ModuleLoader level → `app/frontend/foo/bar.ts` (ts → tsx → js → jsx extension fallback) |
| `./relative` | Relative to parent module, same extension fallback |
| npm package | Deno's `NodeResolver` with real `NpmPackageFolderResolver` + `InNpmPackageChecker` |
| `node:*` | Delegates to Deno's Node builtin compat |
| `import.meta.glob` | Not resolved at module level — removed by Ruby-side codegen |

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

Parallel to `Bundle` but for dev mode:

```ruby
class DevBundle
  def initialize(entry_path, resolve_alias: { '@' => 'app/frontend' })
    @entry_path = entry_path
    @resolve_alias = resolve_alias
    @mtimes = {}  # track all loaded source files for auto-reload
    regenerate_imports!
    load
  end

  def render(data)
    reload_if_changed
    # delegates to same SSR::Deno.native_render as Bundle
  end

  private

  def regenerate_imports!
    # Dir.glob components, write __ssr_imports__.ts
    # Pure Ruby — no Node.js, no tsx
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
  c.dev do
    c.entry :app, Rails.root.join('app/frontend/entrypoints/ssr-app.tsx')
    c.entry :demos, Rails.root.join('app/frontend/entrypoints/ssr-demos.tsx')
    c.alias '@', 'app/frontend'
  end
end
```

Or create a `SSR::Deno::DevBundle` directly in an initializer.

## Dependency graph (dev path only)

```
Ruby: DevBundle.new(entry.tsx)
  → Ruby: generate __ssr_imports__.ts (Dir.glob)
  → Rust: dev_load_entry(entry_path)
    → creates worker with DevModuleLoader
    → deno_ast transpiles .tsx → .js (via transpile feature)
    → Deno's NodeResolver (real NpmPackageFolderResolver) resolves bare specifiers
    → module evaluated, render registered
  → Ruby: bundle.render(data)
    → calls native_render (⚠️ see render routing gap — needs separate FFI or dispatch)
```

## What does NOT change

- `load_bundle()` / `render()` / `render_chunked()` in Rust — untouched
- `Bundle` Ruby class — untouched
- `Config` struct — dev mode passes config inline, not via `Config`
- `NoopModuleLoader` — stays for production
- `NopInNpmPackageChecker` / `NopNpmPackageFolderResolver` — stay for production
- `build_worker()` — untouched, production only
- V8 isolate pool — untouched, dev mode uses separate single-isolate workers
- `Permissions::none_without_prompt()` — stays for production
- `Cargo.toml` default features — `dev-mode` gates Rust code only (no dependency change)
- Test suite — existing tests all test production path, continue passing

## What changes

| Component | Change |
|-----------|--------|
| `ext/ssr_deno/Cargo.toml` | Add `[features]` with optional `dev-mode` flag |
| `ext/ssr_deno/src/dev_module_loader.rs` | **New** — ModuleLoader impl for dev |
| `ext/ssr_deno/src/deno_runtime_wrapper/dev_builder.rs` | **New** — `build_dev_worker()` separate from production builder |
| `ext/ssr_deno/src/deno_runtime_wrapper/dev_load.rs` | **New** — dev entry point loading |
| `ext/ssr_deno/src/lib.rs` | Expose `dev_load_entry` as a `#[magnus::function]` |
| `ext/ssr_deno/src/nop_types.rs` | Add `RealNpmPackageFolderResolver` + `RealInNpmPackageChecker` (or new file `real_npm_types.rs`) |
| `lib/ssr/deno.rb` | Expose `DevBundle` class |
| `lib/ssr/deno/dev_bundle.rb` | **New** — Ruby DevBundle class |
| `lib/ssr/deno/dev_bundle/codegen.rb` | **New** — Ruby-side import map generator |
| `sig/ssr/deno.rbs` | Add `DevBundle` signatures |

## Compile time risk

**No risk.** `deno_ast` (with `transpiling` feature) is already compiled in via `deno_runtime/hmr` → `deno_runtime/transpile` → `deno_ast`. The cost is already paid in every build. The `dev-mode` feature flag gates Rust code only — negligible compared to `deno_ast` itself.

## Known gaps

### HIGH — `native_render` routing for dev workers (blocking)

Dev workers live outside the pool. `native_render` looks up bundle IDs in the pool's registered render functions — it won't find a dev worker. The plan says "delegates to same `SSR::Deno.native_render` as Bundle" but this is incorrect without additional mechanism.

Options:
- **A. Separate FFI function** — add `native_dev_render(worker_handle, payload)` that invokes the dev worker's `globalThis.render` directly. DevBundle holds a reference to its worker.
- **B. Register dev render in pool** — register the dev worker's render function in the pool's function table with a different namespace. Contradicts "pool untouched" constraint.
- **C. Ruby-side dispatch** — DevBundle wraps the dev worker handle and Ruby calls the native function directly, bypassing the pool entirely.

Option A or C is preferred. Implementation order step 1 is to resolve this design before any other Rust work.

### MEDIUM — npm resolver availability

`RealNpmPackageFolderResolver` and `InNpmPackageChecker` need to be confirmed exportable from `deno_runtime = "0.255.0"`. If only traits exist (not concrete types), implement a lightweight `node_modules/` walker that reads `package.json` + resolves bare specifiers to filesystem paths.

### LOW — barrel file eager loading

`__ssr_imports__.ts` via `Dir.glob` re-exports every component file. Deno loads all of them on startup. Mitigation: skip barrel file; rely on the entry's existing explicit imports. The codegen only needs `Dir.glob` if the app uses `import.meta.glob` — strip those lines via regex in Ruby instead.

### LOW — auto-reload ambiguity

Plan mentions both "directory mtime" and "file mtime". Recommended approach: track individual file mtimes in `DevModuleLoader`'s transpile cache. Expose earliest stale mtime to Ruby for `reload_if_changed`. Skip directory mtime entirely.

### LOW — `import.meta.glob` removal

Codegen can't parse TS/JS. For dev mode: (a) document `import.meta.glob` as unsupported, (b) best-effort regex strip `import.meta.glob\(...\)` lines from the generated imports file, (c) warn on detection.

### LOW — Rails helper integration

The `.dev` block in the API example suggests a separate config namespace — `Config.dev_entries?` or similar. Needs specification: does `DevBundle` register in `Config.bundles`, or is there a parallel `Config.dev_bundles`? The helper's `find_bundle!` would need updating for dev bundle lookup.

## Implementation order

1. Design and implement render routing for dev workers (resolve HIGH gap)
2. Add `dev-mode` feature flag to Cargo.toml
3. Verify concrete npm resolver availability: spike to confirm `deno_runtime 0.255.0` exports usable `RealNpmPackageFolderResolver` + `InNpmPackageChecker` (or implement lightweight `node_modules/` walker)
4. Implement `build_dev_worker()` in `dev_builder.rs` — separate builder, relaxed permissions
5. Implement `DevModuleLoader` in `dev_module_loader.rs` — alias resolution, npm resolution, CSS/noop handling, transpiled module cache
6. Add `dev_load_entry()` in `dev_load.rs` — single-isolate worker, ES module evaluation, render registration
7. Expose `dev_load_entry` as a `#[magnus::function]` in `lib.rs` (+ render FFI entry)
8. Write Ruby `DevBundle` class with codegen (`Dir.glob`, `__ssr_imports__.ts` generation)
9. Wire up auto-reload (mtime tracking per loaded module + component dir)
10. Test with side-project: remove Rolldown from Procfile, verify SSR works with `rails s` alone
11. Update plans, docs, stale audit

## Future

- **Oxc minifier** for dev bundle compression (optional, for closer-to-prod simulation)
- **Hot reload** on file change without full re-evaluation (replace module in V8 module map)
- **Error overlay** in Rails for TS/JSX parse errors
