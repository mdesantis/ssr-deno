# Dev-Mode Follow-ups

Deferred cleanups + future enhancements identified during the post-step-9 holistic Rust review. Step 14 (side-project MUI validation) completed 2026-05-14 — all items survive. The **extract-dev-crate** refactor moved to its own plan: [`extract-dev-crate.md`](archived/extract-dev-crate.md) and completed 2026-05-20, moving dev-mode Rust sources into `crates/ssr_deno_dev_mode/`. File paths updated accordingly throughout this document.

## Verification — V8 stack-frame format vs `register_inline` key

[`dev_mode_module_loader.rs:register_source_map`](../ext/ssr_deno/crates/ssr_deno_dev_mode/src/dev_mode_module_loader.rs) keys the global `SsrSourceMapper` under `specifier.as_str()` (e.g. `file:///abs/path/foo.tsx`). `SsrSourceMapper::resolve_line` does exact-string lookup against whatever V8 emits in stack frames.

**Verified (step 14)**: V8 emits `file://` URLs in ES module stack frames. Source maps resolve to `.tsx` originals correctly. No fix needed.

## Performance — read-lock-first in `register_source_map`

[`dev_mode_module_loader.rs:register_source_map`](../ext/ssr_deno/crates/ssr_deno_dev_mode/src/dev_mode_module_loader.rs) acquires the global `SsrSourceMapper` write-lock on every module load — including cache hits. With mtime-skip inside `register_inline`, the inner work is one HashMap lookup, but write-lock acquisition still serializes across ~500 modules per render.

Options:
- Track `registered_in_global_mapper: bool` on `CacheEntry`; skip register on cache hits where flag is true.
- Acquire `read()` first, check existence + mtime; upgrade to `write()` only on miss. Two-phase lock — more code, marginal gain.
- Skip register entirely on cache hits; rely on the original load's registration surviving in the global mapper.

Defer until profiling shows lock contention. Sticky write-lock acquisition on a single-threaded worker is essentially zero contention in practice.

## Performance — `Arc<str>` in `CacheEntry`

[`dev_mode_module_loader.rs:45 (CacheEntry)`](../ext/ssr_deno/crates/ssr_deno_dev_mode/src/dev_mode_module_loader.rs) holds `code: String` and `source_map: Option<String>`. `check_cache` clones both on hit. For a 500-module render with ~10-100 KB per module, that's MBs of allocation per render.

```rust
struct CacheEntry {
    mtime: SystemTime,
    code: Arc<str>,
    source_map: Option<Arc<str>>,
}
```

`ModuleSourceCode::String` accepts `FastString` which has `From<Arc<str>>` ([`fast_string.rs:441`](file:///home/maurizio/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/deno_core-0.400.0/fast_string.rs)). Clone cost drops to a refcount bump.

Constraint: `register_inline` takes `&str`; would still need `.as_ref()` borrow. ✓ trivial.

Defer — measure first. Dev cold-start latency dominated by transpile, not cache hits.

## Refactor — `RefCell` instead of `Mutex` for transpile cache

[`dev_mode_module_loader.rs:52 (DevModeMtimeCache::inner)`](../ext/ssr_deno/crates/ssr_deno_dev_mode/src/dev_mode_module_loader.rs) is `Mutex<HashMap<PathBuf, CacheEntry>>`. The worker is single-threaded (`LocalSet::block_on`), and `Rc<dyn ModuleLoader>` doesn't require `Send + Sync`. `RefCell` would suffice and avoid lock overhead.

Caveat: changing the field type cascades into `check_cache` / `update_cache` borrows. Trivial mechanical change.

Defer — `Mutex` on uncontended single-thread access is ~10ns. Negligible vs transpile.

## ✅ DONE — Hoist `NodeResolutionSys::new(Sys, None)` (2026-05-20)

`build_dev_mode_npm_resolver` now returns a named struct `DevModeNpmResolverParts` including `NodeResolutionSys<Sys>`. Both `build_dev_node_services` and `DevModeModuleLoader::new` consume from it instead of constructing their own. (`dev_mode_npm_resolver.rs` still constructs one internally for `ByonmNpmResolverCreateOptions.sys` — separate role, not hoisted.)

## ✅ DONE — Rename `real_npm_types.rs` → `dev_npm_resolver.rs` → `dev_mode_npm_resolver.rs`

Renamed twice: first `real_npm_types.rs → dev_npm_resolver.rs` (2026-05-14, step 14), then the extract-dev-crate refactor (2026-05-20) moved it into `crates/ssr_deno_dev_mode/` as [`dev_mode_npm_resolver.rs`](../ext/ssr_deno/crates/ssr_deno_dev_mode/src/dev_mode_npm_resolver.rs).

## ✅ DONE — Extract shared `NodeResolverOptions` (2026-05-20)

`dev_mode_builder.rs` and `dev_mode_module_loader.rs` each hardcoded identical `NodeResolverOptions` blocks (import/require condition split for emotion/MUI CJS cycle fix). Extracted to [`dev_mode_npm_resolver.rs:dev_node_resolver_options()`](../ext/ssr_deno/crates/ssr_deno_dev_mode/src/dev_mode_npm_resolver.rs) (`pub(crate)`). Both callers now call `dev_node_resolver_options()`.

## ✅ DONE — `block_on_load_entry` GVL release (2026-05-20)

`native_dev_load_entry` now wraps `block_on_load_entry` in `rb_thread_call_without_gvl`, same pattern as `native_dev_render`. Other Ruby threads no longer stall during the ~1-3s module graph load.

## Future — `native_dev_check_stale` GVL release

`native_dev_check_stale` ([`lib.rs`](../ext/ssr_deno/src/lib.rs)) walks the mtime cache and stats every loaded path. On a 500-module graph that's ~500 syscalls per render call (worst case — `auto_reload` enabled). Holds Ruby GVL throughout. Multi-threaded Puma dev workers stall tens of ms per render.

Acceptable for typical dev. Future: same `rb_thread_call_without_gvl` pattern — the body is FFI-only, no Ruby objects touched.

## Future — Carry transpile cache across auto-reload

Current step-11 strategy: on reload, drop the worker + its `Arc<DevModeMtimeCache>`; spawn a fresh worker with empty cache. Every module re-transpiled even though most are unchanged.

V8's *module map* must be fresh on every reload (cached compiled modules are keyed by URL; reusing them would serve stale code). But the *transpile* cache could survive — for each module whose mtime matches, deno_ast's work is skipped. Only V8's compile pass runs against the (already-transpiled) source.

On a 500-module graph where a single file changed:
- Current: 500 transpiles + 500 V8 compiles
- With cache carry: 1 transpile + 500 V8 compiles

Transpile is the dominant cost. Wiring: store `Arc<DevModeMtimeCache>` on the Ruby `DevModeBundle`; pass into `native_dev_worker_new` (or a new `native_dev_worker_with_cache`) so the new worker reuses the cache. `update_cache` overwrites entries with new mtime, automatically invalidating changed files.

Risk: stale cache entries for files that became invalid (parse error fixed, but cache still holds the OLD valid transpile output keyed under the same mtime). Mitigation: invalidate by content hash, not mtime alone.

Defer — measure reload latency first.

## Future — Source-map registry lifecycle on worker respawn

`SsrSourceMapper` is a global `OnceLock<RwLock<SsrSourceMapper>>` ([`ssr_deno_core/src/source_mapper.rs:global_get_source_mapper`](../ext/ssr_deno/crates/ssr_deno_core/src/source_mapper.rs)). It survives worker drops. Source maps registered under URLs accumulate forever (replaced on same URL re-registration, leaked on stale URLs).

Step 11 (auto-reload) drops + respawns the worker. New module URLs are typically stable (no content hash in dev), so the same keys get overwritten — no growth in steady state. But if the user moves a file or renames a directory, the old URL's map entry stays forever.

Mitigations (consider during step 11):
- Add `SsrSourceMapper::clear_with_prefix(&self, url_prefix: &str)` — call on `DevIsolateHandle::Drop` with the project_root URL prefix.
- Or: each `DevIsolateHandle` tracks registered URLs in a `HashSet<String>`; Drop calls `remove_many`.

For typical dev sessions the leak is bounded by total distinct module URLs visited. Defer.

## Future — Lazy `setup_require`

[`dev_worker.rs:59`](../ext/ssr_deno/src/engine/dev_worker.rs) calls `setup_require` unconditionally during worker init (~10ms cost). If the user's entry uses pure ESM, `globalThis.require` is never consulted — the setup is wasted.

Could lazy-init on first CJS-requiring import. But detection requires hooking into `node_resolver`'s decision path. Disproportionate complexity for a 10ms saving.

Defer — accept the constant cost.

## ✅ DONE — Explicit close on reload (2026-05-20)

`DevModeBundle#close_handle` (private, sets `@handle = nil`) called before `create_worker` in `reload_if_changed`. Old `Arc<DevModeIsolateHandle>` dropped synchronously on handle reassign; worker thread observes channel close on next `rx.recv()`. In-flight renders keep the old worker alive via their captured `Arc` — no premature termination.

## Future — Better `Drop` story for in-flight render on handle drop

If Ruby GCs `DevWorkerHandle` while a render is in-flight (rare — usually the response is held until done):
- `Arc<DevIsolateHandle>` last ref drops → `Sender` drops → channel closes
- Worker thread's current `render::render(...).await` keeps running until V8 finishes (or hits timeout/OOM)
- Worker thread then sees `rx.recv().await` returns None → exits gracefully

So the worker doesn't immediately die — it completes the in-flight render orphaned (reply oneshot is already gone since dropping Handle dropped any holders). Result is silently dropped.

Acceptable but inelegant. Future: `IsolateHandle` thread-safe-handle gives us `terminate_execution()` — could signal cancel on Drop. Defer.

## Future — `DevWorkerMsg` channel capacity

[`dev_handle.rs:49`](../ext/ssr_deno/src/engine/dev_handle.rs) sets `tokio::sync::mpsc::channel::<DevModeWorkerMsg>(1)`. Capacity 1 means concurrent Ruby threads contending for the same DevModeBundle serialize at the channel.

For dev: serialization is correct (single isolate). For prod-pool: round-robin distributes load. Dev's 1-isolate constraint makes capacity-1 the natural choice.

If we ever expose a config knob `dev_isolate_count > 1`, revisit. Defer.

## Future — `import.meta.glob` codegen helper

Plan §"Codegen lifecycle" deferred this. The side-project has a `__ssr_imports__.ts` generated by an external build script (`scripts/build-ssr-imports.ts`); the entry imports it with a plain `import { __ssrComponentsApp } from './__ssr_imports__'`. The dev-mode loader resolves this as a normal relative import — no `import.meta.glob` runtime. If the user's entry used `import.meta.glob(...)` directly at the entry level, the workaround would be a Ruby-side preprocessor that regex-strips it and replaces with explicit static imports built from `Dir.glob`. Only implement if a future entry needs it.

## Future — Inject `__VITE_SOURCE_DIR__` + `import.meta.env` stubs

Step 14 validation revealed the side-project entry hardcodes `/app/frontend` as the source directory and uses a `try/catch` guard for `import.meta.env`. These are Vite-only compile-time defines. Options:

- **A**: inject `globalThis.__VITE_SOURCE_DIR__` in the namespace script (`dev_load.rs`). `import.meta.env` is per-module and can't be injected from outside — needs a module-loader-level transform or a documented stub-import shim.
- **B**: document that user code must guard/define these globals.

Defer — the side-project already has manual workarounds; not blocking.

## Future — Concurrent dev renders via thread-local module loaders

Currently 1 isolate per `DevModeBundle` → 1 render at a time. For a dev workflow with multiple concurrent HTTP requests (eg ParallelHelpers in test, prefork Puma), all renders serialize.

Long-term: per-`DevModeBundle` worker count config. Each worker is independent — separate transpile cache, separate V8 module map, separate `Permissions`. Heavier RAM cost but enables concurrency.

Defer — dev workflows don't usually need this.

## Future — Optional `Arc<dyn CodeCache>` for `v8_code_cache`

[`dev_mode_builder.rs:119`](../ext/ssr_deno/crates/ssr_deno_dev_mode/src/dev_mode_builder.rs) sets `v8_code_cache: None`. Wiring a real `Arc<dyn CodeCache>` (disk-backed) would amortize first-load transpile cost across `rails s` restarts.

Out of scope for v1. Listed in the main plan's [Future](archived/ssr-source-dev-mode.md#future) section.
