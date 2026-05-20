# Dev-Mode — Deferred Work

Extracted from the archived `dev-mode-followups.md` plan. Items that survive but aren't worth implementing yet. Revisit when a concrete need arises.

---

## Performance — read-lock-first in `register_source_map`

[`dev_mode_module_loader.rs:register_source_map`](../ext/ssr_deno/crates/ssr_deno_dev_mode/src/dev_mode_module_loader.rs) acquires the global `SsrSourceMapper` write-lock on every module load — including cache hits. With mtime-skip inside `register_inline`, the inner work is one HashMap lookup, but write-lock acquisition still serializes across ~500 modules per render.

Options:
- Track `registered_in_global_mapper: bool` on `CacheEntry`; skip register on cache hits where flag is true.
- Acquire `read()` first, check existence + mtime; upgrade to `write()` only on miss. Two-phase lock — more code, marginal gain.
- Skip register entirely on cache hits; rely on the original load's registration surviving in the global mapper.

## Refactor — `RefCell` instead of `Mutex` for transpile cache

[`dev_mode_module_loader.rs:52 (DevModeMtimeCache::inner)`](../ext/ssr_deno/crates/ssr_deno_dev_mode/src/dev_mode_module_loader.rs) is `Mutex<HashMap<PathBuf, CacheEntry>>`. The worker is single-threaded (`LocalSet::block_on`), and `Rc<dyn ModuleLoader>` doesn't require `Send + Sync`. `RefCell` would suffice and avoid lock overhead.

Caveat: changing the field type cascades into `check_cache` / `update_cache` borrows. Trivial mechanical change.

## Future — Carry transpile cache across auto-reload

Current step-11 strategy: on reload, drop the worker + its `Arc<DevModeMtimeCache>`; spawn a fresh worker with empty cache. Every module re-transpiled even though most are unchanged.

V8's *module map* must be fresh on every reload (cached compiled modules are keyed by URL; reusing them would serve stale code). But the *transpile* cache could survive — for each module whose mtime matches, deno_ast's work is skipped. Only V8's compile pass runs against the (already-transpiled) source.

On a 500-module graph where a single file changed:
- Current: 500 transpiles + 500 V8 compiles
- With cache carry: 1 transpile + 500 V8 compiles

Wiring: store `Arc<DevModeMtimeCache>` on the Ruby `DevModeBundle`; pass into `native_dev_worker_new` (or a new `native_dev_worker_with_cache`) so the new worker reuses the cache. `update_cache` overwrites entries with new mtime, automatically invalidating changed files.

Risk: stale cache entries for files that became invalid (parse error fixed, but cache still holds the OLD valid transpile output keyed under the same mtime). Mitigation: invalidate by content hash, not mtime alone.

## Future — Source-map registry lifecycle on worker respawn

`SsrSourceMapper` is a global `OnceLock<RwLock<SsrSourceMapper>>` ([`ssr_deno_core/src/source_mapper.rs:global_get_source_mapper`](../ext/ssr_deno/crates/ssr_deno_core/src/source_mapper.rs)). It survives worker drops. Source maps registered under URLs accumulate forever (replaced on same URL re-registration, leaked on stale URLs).

Step 11 (auto-reload) drops + respawns the worker. New module URLs are typically stable (no content hash in dev), so the same keys get overwritten — no growth in steady state. But if the user moves a file or renames a directory, the old URL's map entry stays forever.

Mitigations:
- Add `SsrSourceMapper::clear_with_prefix(&self, url_prefix: &str)` — call on `DevIsolateHandle::Drop` with the project_root URL prefix.
- Or: each `DevIsolateHandle` tracks registered URLs in a `HashSet<String>`; Drop calls `remove_many`.

## Future — Lazy `setup_require`

[`dev_worker.rs:59`](../ext/ssr_deno/src/engine/dev_worker.rs) calls `setup_require` unconditionally during worker init (~10ms cost). If the user's entry uses pure ESM, `globalThis.require` is never consulted.

Could lazy-init on first CJS-requiring import. But detection requires hooking into `node_resolver`'s decision path. Disproportionate complexity for a 10ms saving.

## Future — Better `Drop` story for in-flight render on handle drop

If Ruby GCs `DevWorkerHandle` while a render is in-flight (rare — usually the response is held until done):
- `Arc<DevIsolateHandle>` last ref drops → `Sender` drops → channel closes
- Worker thread's current `render::render(...).await` keeps running until V8 finishes (or hits timeout/OOM)
- Worker thread then sees `rx.recv().await` returns None → exits gracefully

So the worker doesn't immediately die — it completes the in-flight render orphaned (reply oneshot is already gone since dropping Handle dropped any holders). Result is silently dropped.

`IsolateHandle` thread-safe-handle gives us `terminate_execution()` — could signal cancel on Drop.

## Future — `DevWorkerMsg` channel capacity

[`dev_handle.rs:49`](../ext/ssr_deno/src/engine/dev_handle.rs) sets `tokio::sync::mpsc::channel::<DevModeWorkerMsg>(1)`. Capacity 1 means concurrent Ruby threads contending for the same DevModeBundle serialize at the channel.

For dev: serialization is correct (single isolate). For prod-pool: round-robin distributes load. If we ever expose a config knob `dev_isolate_count > 1`, revisit.

## Future — `import.meta.glob` codegen helper

If the user's entry used `import.meta.glob(...)` directly at the entry level, the workaround would be a Ruby-side preprocessor that regex-strips it and replaces with explicit static imports built from `Dir.glob`. Only implement if a future entry needs it.

## Future — Inject `__VITE_SOURCE_DIR__` + `import.meta.env` stubs

Step 14 validation revealed the side-project entry hardcodes `/app/frontend` as the source directory and uses a `try/catch` guard for `import.meta.env`. These are Vite-only compile-time defines.

Options:
- **A**: inject `globalThis.__VITE_SOURCE_DIR__` in the namespace script (`dev_load.rs`). `import.meta.env` is per-module and can't be injected from outside — needs a module-loader-level transform or a documented stub-import shim.
- **B**: document that user code must guard/define these globals.

## Future — Concurrent dev renders via thread-local module loaders

Currently 1 isolate per `DevModeBundle` → 1 render at a time. For a dev workflow with multiple concurrent HTTP requests (eg ParallelHelpers in test, prefork Puma), all renders serialize.

Long-term: per-`DevModeBundle` worker count config. Each worker is independent — separate transpile cache, separate V8 module map, separate `Permissions`. Heavier RAM cost but enables concurrency.

## Future — Optional `Arc<dyn CodeCache>` for `v8_code_cache`

[`dev_mode_builder.rs:119`](../ext/ssr_deno/crates/ssr_deno_dev_mode/src/dev_mode_builder.rs) sets `v8_code_cache: None`. Wiring a real `Arc<dyn CodeCache>` (disk-backed) would amortize first-load transpile cost across `rails s` restarts.
