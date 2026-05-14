# Dev-Mode Follow-ups

Deferred cleanups + future enhancements identified during the post-step-9 holistic Rust review. None block step 10+; revisit after the side-project end-to-end test (step 12) reveals real-world hotspots.

## Verification — V8 stack-frame format vs `register_inline` key

[`dev_module_loader.rs:register_source_map`](../ext/ssr_deno/src/dev_module_loader.rs) keys the global `SsrSourceMapper` under `specifier.as_str()` (e.g. `file:///abs/path/foo.tsx`). `SsrSourceMapper::resolve_line` does exact-string lookup against whatever V8 emits in stack frames.

**Untested**: V8's actual format for ES module frames hasn't been observed in this codebase. Likely matches (`at file:///abs/path/foo.tsx:N:N`) but worth confirming the first time step 12 runs.

If V8 emits a stripped path (`/abs/path/foo.tsx` without `file://`), stack frames won't resolve. Fixes:
- A: register under both URL and path keys
- B: normalize at lookup time in `resolve_line` (strip `file://`)

Test by deliberately throwing inside a `.tsx` and inspecting `BundleLoad` / `Render` error message format.

## Performance — read-lock-first in `register_source_map`

[`dev_module_loader.rs:register_source_map`](../ext/ssr_deno/src/dev_module_loader.rs) acquires the global `SsrSourceMapper` write-lock on every module load — including cache hits. With mtime-skip inside `register_inline`, the inner work is one HashMap lookup, but write-lock acquisition still serializes across ~500 modules per render.

Options:
- Track `registered_in_global_mapper: bool` on `CacheEntry`; skip register on cache hits where flag is true.
- Acquire `read()` first, check existence + mtime; upgrade to `write()` only on miss. Two-phase lock — more code, marginal gain.
- Skip register entirely on cache hits; rely on the original load's registration surviving in the global mapper.

Defer until profiling shows lock contention. Sticky write-lock acquisition on a single-threaded worker is essentially zero contention in practice.

## Performance — `Arc<str>` in `CacheEntry`

[`dev_module_loader.rs:CacheEntry`](../ext/ssr_deno/src/dev_module_loader.rs) holds `code: String` and `source_map: Option<String>`. `check_cache` clones both on hit. For a 500-module render with ~10-100 KB per module, that's MBs of allocation per render.

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

[`dev_module_loader.rs:cache`](../ext/ssr_deno/src/dev_module_loader.rs) is `Mutex<HashMap<PathBuf, CacheEntry>>`. The worker is single-threaded (`LocalSet::block_on`), and `Rc<dyn ModuleLoader>` doesn't require `Send + Sync`. `RefCell` would suffice and avoid lock overhead.

Caveat: changing the field type cascades into `check_cache` / `update_cache` borrows. Trivial mechanical change.

Defer — `Mutex` on uncontended single-thread access is ~10ns. Negligible vs transpile.

## Refactor — Hoist `NodeResolutionSys::new(Sys, None)`

[`dev_builder.rs:46`](../ext/ssr_deno/src/deno_runtime_wrapper/dev_builder.rs) and [`dev_module_loader.rs:87`](../ext/ssr_deno/src/dev_module_loader.rs) each construct their own `NodeResolutionSys<Sys>` — cheap wrapper but redundant.

Extend `build_dev_npm_resolver` return tuple to include `NodeResolutionSys<Sys>`:

```rust
pub fn build_dev_npm_resolver(
    project_root: &Path,
) -> (
    ByonmInNpmPackageChecker,
    ByonmNpmResolver<Sys>,
    PackageJsonResolverRc<Sys>,
    NodeResolutionSys<Sys>,
)
```

Callers `.clone()` the `NodeResolutionSys` if both need owned values (it's `Clone`).

Tradeoff: tuple grows to 4-arity. Could switch to a named struct `DevNpmResolverParts { ... }`. Defer.

## Cleanup — `build_dev_npm_module_loader` unused param + comment gap

[`real_npm_types.rs:62`](../ext/ssr_deno/src/real_npm_types.rs) `build_dev_npm_module_loader(_project_root: &Path, ...)` takes the project root but never uses it — the `ByonmNpmResolver` arg already has it baked in. Drop the param or use it (eg pass to a future `ParsedSourceCache` keyed on file paths).

Same file, line 66: `DenoInNpmPackageChecker::Byonm(ByonmInNpmPackageChecker)` wraps the raw checker in the enum variant. The wrap is required because `NpmModuleLoader`'s generic param is `DenoInNpmPackageChecker` (enum) but `build_dev_node_services` in `dev_builder.rs` uses raw `ByonmInNpmPackageChecker`. The two checker types thread through the type system with different generics — easy to confuse on read. Add a one-line comment explaining the wrap vs raw choice at each construction site.

## Rename — `real_npm_types.rs` → `dev_npm_resolver.rs`

[`real_npm_types.rs`](../ext/ssr_deno/src/real_npm_types.rs) — name dates back to the plan's pre-spike phase when we expected to implement a walker. Now it's just a Byonm builder. `dev_npm_resolver.rs` is more descriptive.

Risk-free rename:
- File rename
- `mod real_npm_types;` → `mod dev_npm_resolver;` in `lib.rs`
- Two `use crate::real_npm_types::build_dev_npm_resolver` → `use crate::dev_npm_resolver::build_dev_npm_resolver`

Defer — cosmetic.

## Future — `block_on_load_entry` GVL release

[`lib.rs:native_dev_load_entry`](../ext/ssr_deno/src/lib.rs) blocks the Ruby GVL for the duration of `block_on_load_entry`, which awaits load + transpile of the full module graph (~1-3s on a deep MUI tree). Other Ruby threads stall.

Acceptable in dev because:
- Load happens once per worker lifetime (or on auto-reload respawn).
- Puma in dev is typically single-threaded.

Future: wrap in `rb_thread_call_without_gvl` like [`native_dev_render`](../ext/ssr_deno/src/lib.rs). Pattern is identical — box `(handle, entry_path, aliases)`, callback calls `block_on_load_entry`. Defer until multi-thread dev becomes a real use case.

## Future — `native_dev_check_stale` GVL release

`native_dev_check_stale` ([`lib.rs`](../ext/ssr_deno/src/lib.rs)) walks the mtime cache and stats every loaded path. On a 500-module graph that's ~500 syscalls per render call (worst case — `auto_reload` enabled). Holds Ruby GVL throughout. Multi-threaded Puma dev workers stall tens of ms per render.

Acceptable for typical dev. Future: same `rb_thread_call_without_gvl` pattern — the body is FFI-only, no Ruby objects touched.

## Future — Carry transpile cache across auto-reload

Current step-11 strategy: on reload, drop the worker + its `Arc<DevMtimeCache>`; spawn a fresh worker with empty cache. Every module re-transpiled even though most are unchanged.

V8's *module map* must be fresh on every reload (cached compiled modules are keyed by URL; reusing them would serve stale code). But the *transpile* cache could survive — for each module whose mtime matches, deno_ast's work is skipped. Only V8's compile pass runs against the (already-transpiled) source.

On a 500-module graph where a single file changed:
- Current: 500 transpiles + 500 V8 compiles
- With cache carry: 1 transpile + 500 V8 compiles

Transpile is the dominant cost. Wiring: store `Arc<DevMtimeCache>` on the Ruby `DevModeBundle`; pass into `native_dev_worker_new` (or a new `native_dev_worker_with_cache`) so the new worker reuses the cache. `update_cache` overwrites entries with new mtime, automatically invalidating changed files.

Risk: stale cache entries for files that became invalid (parse error fixed, but cache still holds the OLD valid transpile output keyed under the same mtime). Mitigation: invalidate by content hash, not mtime alone.

Defer — measure reload latency first.

## Future — Source-map registry lifecycle on worker respawn

`SsrSourceMapper` is a global `OnceLock<RwLock<SsrSourceMapper>>` ([`lib.rs:get_source_mapper`](../ext/ssr_deno/src/lib.rs)). It survives worker drops. Source maps registered under URLs accumulate forever (replaced on same URL re-registration, leaked on stale URLs).

Step 11 (auto-reload) drops + respawns the worker. New module URLs are typically stable (no content hash in dev), so the same keys get overwritten — no growth in steady state. But if the user moves a file or renames a directory, the old URL's map entry stays forever.

Mitigations (consider during step 11):
- Add `SsrSourceMapper::clear_with_prefix(&self, url_prefix: &str)` — call on `DevIsolateHandle::Drop` with the project_root URL prefix.
- Or: each `DevIsolateHandle` tracks registered URLs in a `HashSet<String>`; Drop calls `remove_many`.

For typical dev sessions the leak is bounded by total distinct module URLs visited. Defer.

## Future — Lazy `setup_require`

[`dev_worker.rs:55-62`](../ext/ssr_deno/src/deno_runtime_wrapper/dev_worker.rs) calls `setup_require` unconditionally during worker init (~10ms cost). If the user's entry uses pure ESM, `globalThis.require` is never consulted — the setup is wasted.

Could lazy-init on first CJS-requiring import. But detection requires hooking into `node_resolver`'s decision path. Disproportionate complexity for a 10ms saving.

Defer — accept the constant cost.

## Cleanup — Explicit close for stale workers on auto-reload

`DevModeBundle#reload_if_changed` ([`dev_mode_bundle.rb`](../lib/ssr/deno/dev_mode_bundle.rb)) reassigns `@handle = SSR::Deno.native_dev_worker_new(...)`. The old `DevWorkerHandle` Ruby object becomes GC-eligible, but the Rust `Arc<DevIsolateHandle>` (and the V8 isolate ~64 MB + worker thread) only drops when Ruby GC reclaims the wrapper.

Typical dev (1-2 reloads/min): GC keeps up; no observable buildup.

Rapid-save bursts (user mass-saves 10 files via editor "save all"): several stale workers may co-exist for tens of seconds until GC fires. Each ~64 MB V8 heap. Peak RSS spikes.

Fixes:
- **A**: explicit `close` method on `DevModeBundle` — call before reassigning `@handle`. Old Arc dropped synchronously; worker thread observes channel close immediately.
- **B**: store `IsolateHandle::thread_safe_handle()` in `DevWorkerHandle`; Drop impl triggers `terminate_execution()` to force worker thread to fast-exit even before the channel closes.

A is the Ruby-friendly path. Reload flow becomes:
```ruby
def reload_if_changed
  ...
  @_bundle_mutex.synchronize do
    ...
    close_handle(@handle)  # explicit drain before reassign
    create_worker
    load_entry
  end
end
```

Cleaner GC-independent lifecycle. Defer until rapid-reload RSS spikes show up in practice.

## Future — Better `Drop` story for in-flight render on handle drop

If Ruby GCs `DevWorkerHandle` while a render is in-flight (rare — usually the response is held until done):
- `Arc<DevIsolateHandle>` last ref drops → `Sender` drops → channel closes
- Worker thread's current `render::render(...).await` keeps running until V8 finishes (or hits timeout/OOM)
- Worker thread then sees `rx.recv().await` returns None → exits gracefully

So the worker doesn't immediately die — it completes the in-flight render orphaned (reply oneshot is already gone since dropping Handle dropped any holders). Result is silently dropped.

Acceptable but inelegant. Future: `IsolateHandle` thread-safe-handle gives us `terminate_execution()` — could signal cancel on Drop. Defer.

## Future — `DevWorkerMsg` channel capacity

[`dev_handle.rs:44`](../ext/ssr_deno/src/deno_runtime_wrapper/dev_handle.rs) sets `tokio::sync::mpsc::channel::<DevWorkerMsg>(1)`. Capacity 1 means concurrent Ruby threads contending for the same DevModeBundle serialize at the channel.

For dev: serialization is correct (single isolate). For prod-pool: round-robin distributes load. Dev's 1-isolate constraint makes capacity-1 the natural choice.

If we ever expose a config knob `dev_isolate_count > 1`, revisit. Defer.

## Future — `import.meta.glob` codegen helper

Plan §"Codegen lifecycle" deferred this. If the user's entry uses Vite's `import.meta.glob(...)`, dev mode either:
- Errors at parse time (`deno_ast` doesn't know `import.meta.glob` semantics — actually it does parse it but returns it as a runtime call)
- Returns `undefined` at runtime, breaks at first use

A Ruby-side preprocessor that regex-strips `import.meta.glob(...)` and replaces with explicit static imports built from `Dir.glob` is the documented mitigation. Implement only if the side-project test (step 12) needs it.

## Future — Concurrent dev renders via thread-local module loaders

Currently 1 isolate per `DevModeBundle` → 1 render at a time. For a dev workflow with multiple concurrent HTTP requests (eg ParallelHelpers in test, prefork Puma), all renders serialize.

Long-term: per-`DevModeBundle` worker count config. Each worker is independent — separate transpile cache, separate V8 module map, separate `Permissions`. Heavier RAM cost but enables concurrency.

Defer — dev workflows don't usually need this.

## Future — Optional `Arc<dyn CodeCache>` for `v8_code_cache`

[`dev_builder.rs:113`](../ext/ssr_deno/src/deno_runtime_wrapper/dev_builder.rs) sets `v8_code_cache: None`. Wiring a real `Arc<dyn CodeCache>` (disk-backed) would amortize first-load transpile cost across `rails s` restarts.

Out of scope for v1. Listed in the main plan's [Future](ssr-source-dev-mode.md#future) section.
