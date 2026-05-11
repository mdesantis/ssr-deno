# ESM Multi-Chunk Bundle Support — PoC

## Status: PROOF OF CONCEPT — not production-ready, tests not fully green

## Context

Ruby gem `ssr-deno` embeds Deno V8 via Rust native extension. Until now, every
bundle must be a single self-contained JS file (`noExternal: true`). Standard
Vite SSR builds produce multi-chunk ESM output: an entry file with
`import`/`export` statements referencing sibling chunk files. These cannot be
loaded via `execute_script` (script mode).

Goal: support multi-chunk ESM bundles where all chunks are local files within
the bundle's output directory. **Experimental, opt-in via `esm: true` flag.
Default behavior unchanged.**

Scope: named ESM export (`export function render`), local chunk files only.
External npm bare specifiers and dynamic `import()` at render time: out of scope.

## API

```ruby
# Bundle API — new esm: keyword, default false
SSR::Deno::Bundle.new('dist/server/entry.js', esm: true)
SSR::Deno::Bundle.register(:application, 'dist/server/entry.js', esm: true)

# Rails railtie — hash form or scalar form
config.ssr_deno.bundles = {
  application: { path: 'dist/server/entry.js', esm: true },
  legacy:      'dist/legacy/ssr.js',   # scalar = esm: false
}

# RactorPool
SSR::Deno::RactorPool.new(bundle_path: 'dist/server/entry.js', esm: true)
```

`esm: false` (default): entire new code path never entered. Zero risk to existing users.

## Architecture

### Current (script) flow — unchanged for esm: false
- Pool reads entry file → broadcasts `Arc<str>` to all workers
- Each worker: IIFE wrap → `execute_script` → registers `globalThis.render` in `__ssr_bundles[id]`
- Render: `execute_script` calling `__ssr_bundles[id].render(data)` — same for both paths

### New (ESM) flow — esm: true only
1. Ruby passes `is_esm: true` to `native_load_bundle`
2. Pool broadcasts `is_esm: true` in `WorkerMsg::LoadBundle`
3. Each worker: registers allowed dir + synthetic boot module in `EsmLoaderState`,
   calls `load_esm_bundle_in_worker`
4. Boot module (synthetic, per bundle_id+version):
   ```js
   import { render } from 'file:///abs/path/to/entry.js?v=1';
   if (typeof globalThis.__ssr_bundles === 'undefined') { globalThis.__ssr_bundles = {}; }
   globalThis.__ssr_bundles["bundle-id"] = { render };
   ```
5. `preload_side_module(boot_url)` → `evaluate_module` → `run_event_loop(false)` → verify render
6. **Render path unchanged** — `__ssr_bundles[id].render(data)` identical for both paths

`preload_side_module` is used (not `preload_main_module`) because Deno only
allows one main module per worker, but multiple ESM bundles may be loaded into
the same worker over its lifetime.

## Files Changed

### New
- `ext/ssr_deno/src/deno_runtime_wrapper/esm_loader.rs` — `EsmLoaderState` + `FilesystemModuleLoader`
- `test/fixtures/esm-entry.js` — ESM entry that imports from chunk, exports `render`
- `test/fixtures/esm-chunk.js` — chunk with named `greet` export
- `test/fixtures/esm-admin-entry.js` — second ESM entry for two-bundle coexistence test
- `test/fixtures/esm-no-render-entry.js` — ESM entry without `render` export (error test)
- `test/fixtures/esm-escape-entry.js` — ESM entry importing outside bundle dir (security test)
- `test/ssr/test_deno_bundle_esm.rb` — ESM bundle test suite

### Deleted
- `ext/ssr_deno/src/node_builtin_loader.rs` — replaced by `FilesystemModuleLoader`

### Modified — Rust
- `ext/ssr_deno/src/deno_runtime_wrapper/mod.rs` — added `pub(crate) mod esm_loader;`
- `ext/ssr_deno/src/deno_runtime_wrapper/types.rs` — added `is_esm: bool` to `WorkerMsg::LoadBundle`
- `ext/ssr_deno/src/deno_runtime_wrapper/pool.rs` — `load_bundle(…, is_esm: bool)`, threads flag through broadcast
- `ext/ssr_deno/src/deno_runtime_wrapper/builder.rs` — returns `(MainWorker, Rc<RefCell<EsmLoaderState>>)`; always uses `FilesystemModuleLoader`
- `ext/ssr_deno/src/deno_runtime_wrapper/worker.rs` — holds `loader_state`; branches on `is_esm`; adds `load_esm_bundle_in_worker`
- `ext/ssr_deno/src/lib.rs` — `native_load_bundle` gains `is_esm: bool` param; Magnus arity 2→3; removed `mod node_builtin_loader`

### Modified — Ruby
- `lib/ssr/deno/bundle.rb` — `initialize(bundle_path, esm: false)`; stores `@esm`; passes to `load`; `create_bundles!` reads `cfg[:esm]`
- `lib/ssr/deno/ractor_pool.rb` — `initialize(…, esm: false)`; stores `@esm`; passes to `init_pool` and into Ractor args
- `lib/ssr/deno/ractor_pool/worker.rb` — `loop_body(path, auto, esm: false)`; threads `esm` through `maybe_reload`, `dispatch`, `handle_reload`
- `lib/ssr/deno/rails/railtie.rb` — bundle config accepts String or `{ path:, esm: }` Hash
- `test/support/fixture_paths.rb` — added `ESM_ENTRY`, `ESM_ADMIN_ENTRY`, `ESM_NO_RENDER_ENTRY`, `ESM_ESCAPE_ENTRY`
- `sig/ssr/deno.rbs` — updated `Bundle.new`, `Bundle.register`, `RactorPool.new`, `native_load_bundle`, `Worker` module signatures

## Implementation Report

### ✅ Done
- All Rust code written and **compiles clean** (`bundle exec rake compile` passes)
- All Ruby code written (bundle.rb, ractor_pool.rb, ractor_pool/worker.rb, railtie.rb)
- Test fixtures created
- RBS signatures updated
- First test run shows 3/7 ESM tests passing:
  - `test_script_bundle_still_works_with_esm_false` ✅ (regression — script path unchanged)
  - `test_esm_bundle_missing_render_export_raises` ✅ (error detected correctly)
  - `test_esm_bundle_import_outside_dir_raises` ✅ (security boundary enforced)

### ❌ Failing — root cause identified + fix applied

**Error:** `Trying to create "main" module (…v=2) when one already exists (…v=1)`

**Cause:** `preload_main_module` can only be called once per `MainWorker`. Tests
share one pool (OnceLock). Second ESM bundle load on the same worker → error.

**Fix applied (not yet verified):**
Changed `worker.preload_main_module(&boot_spec)` → `worker.preload_side_module(&boot_spec)`
in `load_esm_bundle_in_worker`. Side modules have no "only one" restriction.

### ◐ Pending
- Recompile after `preload_side_module` fix + run `bundle exec rake test`
- Full pipeline: `bundle exec rake`
- Stale docs audit: `docs/architecture.md`, `docs/compatibility.md`, `README.md`
- `CHANGELOG.md` — Unreleased entry

## Known Limitations (to document in docs/)

- **Reload + chunk cache:** entry file versioned (`?v=N`) → re-evaluated on reload;
  transitive chunk imports resolve to unversioned `file://` URLs → V8 may serve
  cached version. Chunk changes may not take effect without process restart.
- **External npm:** bare specifiers (e.g. `import 'react'`) rejected. All npm deps
  must still be bundled.
- **Dynamic `import()` at render time:** untested.
- **CSS/asset imports in SSR entry:** JS only; CSS imports will fail.

## Resume Checklist

1. `bundle exec rake compile` (already clean, but verify after preload_side_module change)
2. `bundle exec rake test` — expect all 7 ESM tests + full suite green
3. `bundle exec rake` — full gate (compile + cargo test + Vite build + Ruby tests + RuboCop + SimpleCov + RBS)
4. Docs audit + CHANGELOG entry
5. Commit (use caveman-commit skill)
