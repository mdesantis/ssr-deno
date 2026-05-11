## Unreleased

### Added
- **`SSR::Deno::Config`** — dedicated module for runtime settings. `SSR::Deno.max_heap_size_mb=` et al. moved to `SSR::Deno::Config.max_heap_size_mb=`. Thread-safe (Mutex). Native FFI methods stay on `SSR::Deno`.
- **`SSR::Deno::HeapStats`** — dedicated module for heap statistics. `SSR::Deno.heap_stats` → `SSR::Deno::HeapStats.fetch`, `heap_stats!` → `fetch!`.
- **CI now runs on push to main and pull requests** — was manual-only (workflow_dispatch) due to 3h+ V8 builds. With sccache + mold, builds take ~20m.
- **Ruby 3.3 and 3.4 added to CI matrix** — alongside 4.0. Cache keys include Ruby version to prevent cross-ABI artifact corruption.
- **`vite-hmr-ssr-app` sample** — added to samples table and builds.
- **RactorPool documented** — `SSR::Deno::RactorPool` API now covered in README.
- **README restructured** — Rails docs consolidated under "Using with Rails", CSP nonce extracted to standalone render usage section, all config under single Configuration section with Runtime settings / Rails settings / Heap Statistics subsections.

### Fixed
- **`Instrumenter.instrument` called without a block no longer raises `LocalJumpError`** — no-AS mode now uses `elsif block_given?` and yields the payload hash to the block (matching AS behaviour).
- **`find_bundle!` registry read is now thread-safe** — always calls `create_bundles!` before reading, eliminating the concurrent read-during-`transform_values!` window.
- **`ssr_render` raises `ArgumentError` on unknown options** — typos like `raw_ouputput: true` now fail immediately with the unknown key named, instead of silently passing to `bundle.render`.
- **Bundle reload thread safety** — `Bundle#reload` uses Mutex for mtime check/write. Production read path drops the mutex (GVL-protected) for zero overhead.
- **Title corrected** — project name uses `SSR::Deno` consistently.

### Removed
- **BREAKING:** `SSR::Deno.max_heap_size_mb`, `isolate_pool_size`, `render_timeout_ms`, `node_builtins_enabled` setters/getters removed from `SSR::Deno`. Use `SSR::Deno::Config.*` instead.
- **BREAKING:** `SSR::Deno.heap_stats` / `SSR::Deno.heap_stats!` removed from `SSR::Deno`. Use `SSR::Deno::HeapStats.fetch` / `fetch!` instead.

## [0.1.0-alpha.6] - 2026-05-10

### Added
- **`SSR::Deno::RactorPool`** — parallel SSR via Ractors. Each Ractor runs its own GVL, so native FFI calls execute concurrently without serialization. Bypasses Bundle + ActiveSupport::Notifications (Ractor-unsafe). Supports `render`, `render_chunks`, `reload`, auto_reload, and graceful shutdown.
- **`vite-hmr-ssr-app` sample** — minimal Vite SSR sample used by the HMR integration test.
- **GVL release during native_render** — `rb_thread_call_without_gvl` releases the Ruby Global VM Lock during the blocking channel recv in `native_render`. Thread-based Puma now benefits from multiple isolates (previously only helpful for Ractors). Up to 2.4x throughput improvement with 4 threads.
- **HMR integration test** — verifies that `auto_reload` picks up Vite SSR bundle rebuilds (modify source → rebuild → auto-detect via mtime → render updated content).

### Fixed
- **Bundle reload could fail with libraries using module-level singleton guards** (e.g. MUI X Charts) — `var` declarations at global scope persist across repeated `execute_script` calls, causing double-instantiation errors. Bundle code is now wrapped in an IIFE so each reload gets fresh `var` bindings.
- **Scripts: percentile calculation corrected for even-sized arrays** — uses nearest-rank via `.ceil - 1` instead of index truncation.
- **Scripts: `--node-builtins` / `--no-node-builtins` flags added** — overrides the heuristic auto-detect regex.
- **Auto-reload was not updating bundle code** — `load_bundle_in_worker` had a `HashSet` cache (`loaded_paths`) that prevented re-evaluation of the same `(bundle_path, bundle_id)` pair, plus the namespace registration script had an early-return guard for already-registered bundle IDs. Both guards are now removed; the Ruby layer's `mtime` check is the authoritative reload gate.
- Railtie: wire `config.ssr_deno.render_timeout_ms` to `SSR::Deno.render_timeout_ms=` setter. Previously only settable via env var or direct call before pool init.
- `apply_integer_env` warning now includes the error message (e.g. "Render timeout must be at least 100ms") instead of generic "Invalid integer".
- `apply_bool_env` warns on unrecognised values (e.g. `SSR_DENO_NODE_BUILTINS_ENABLED=treu`) instead of silently treating as false, and now returns early without calling the setter (preserving prior value).
- `reload_if_changed` documents thread-safety limitation with comment.
- `Dir.mktmpdir` temp dirs cleaned up after each test (was leaking in test helpers).
- Dead code removed from `scripts/performance.rb`: no-op `isolate_pool_size` getter call, unsynchronized unused `timings` array in multi-thread mode.
- `heap_stats` subscriber guarded by `config.ssr_deno.enabled` check for symmetry with `init_bundles`.
- **Railtie: `node_builtins_enabled` setter no longer silently skipped when set to `false`** — changed guard from `if` to `unless .nil?` so explicit `false` is forwarded to the runtime.
- **`Bundle#render` now sets `payload[:error]` on failure** — `render.ssr_deno` event carries `:error` key when native render raises, so subscribers (event logger, heap sampler) see the failure.
- **`Bundle#render_chunks` now fires `render.ssr_deno` instrumentation** — wrapped `native_render_chunks` call so heap stats and event logging work for chunked renders too.
- **Railtie heap_stats rescue broadened** to catch `JSON::ParserError` alongside `SSR::Deno::Error`.
- **Thread leak fixed in `test_create_bundles_outer_guard`** — `locked_mutex` unlocked in `ensure` block to prevent zombie thread on timeout.

### Removed
- **BREAKING:** `railties` is no longer a runtime dependency. Users who rely on `ssr-deno` pulling in `railties` transitively must now add `gem "railties"` to their Gemfile explicitly. The Rails integration (`require: "ssr/deno/rails"`) raises a clear `LoadError` if `railties` is missing.
- **BREAKING:** `ssr_render` no longer calls `.html_safe` on String results. The helper returns raw bundle output as-is — the caller (app view) is responsible for marking output safe. CSR fallback is plain `''` instead of `''.html_safe`.

### Added
- `ssr_render.ssr_deno` instrumentation event — wraps end-to-end render including bundle lookup, JSON serialization, V8 execution, and error handling. Payload includes `bundle_name` and `error` (on failure). Timing measured via `ActiveSupport::Notifications` block form.
- `config.ssr_deno.raise_on_bundle_error` — when true (default in dev/test), `BundleNotFoundError` at render raises. When false (production), caught and logged with CSR fallback (empty string). Defaults to `!Rails.env.production?`.
- `cargo fmt --check` added to default Rake task and CI pipeline.
- `test:rails` test suite — Rails integration tests (Railtie, Helper) now run via Combustion. Replaces dead hand-crafted `test/dummy/` approach. 8 tests covering Railtie config, Helper inclusion in ActionView::Base, registry state, and instrumentation events. Run with `bundle exec rake test:rails` or as part of `rake test`.
- Puma integration tests: single mode (in-process, coverage-tracked) and clustered mode (subprocess, 2 workers, preload_app! + lazy Bundle) via `test:puma` suite. Verifies that `Bundle.new` deferred to first request works correctly after fork. Covers the V8 TLS limitation (isolates cannot be created after fork).
- `Bundle.create_bundles!` class method — bundle creation for Puma `on_worker_boot` compatibility. `InstallGenerator` now appends the `on_worker_boot` hook to `config/puma.rb`.
- Railtie: wire `config.ssr_deno.node_builtins_enabled` to `SSR::Deno.node_builtins_enabled=` setter.

### Changed
- **BREAKING:** `isolate_pool_size` default changed from `0` (auto-detect from CPU count) to `1`. Performance benchmarks show that Ruby threads do not benefit from multiple isolates due to GVL serialization — only Ractors achieve true parallelism. Users with Ractor-based concurrency should explicitly set `isolate_pool_size` to match their pool needs.
- **BREAKING:** Removed `SSR::Deno::Bundle::Registry` class. `Bundle.registry` is now a plain `Hash` — stores config hashes before `create_bundles!` and `Bundle` instances after. Eliminates `Bundle.deferred_bundles` ivar. `create_bundles!` uses `transform_values!` (no separate register step). All callers updated to use `is_a?(Bundle)` checks and direct hash access.
- Railtie: remove unnecessary `after: 'ssr_deno.subscribe_events'` dependency from `heap_stats` initializer. Both initializers only register event subscription callbacks — neither emits events during initialization — so ordering is irrelevant.
- Railtie `init_bundles` now defers `Bundle.new` to `on_worker_boot` (Puma clustered) or first render (single mode). Bundle configs stored via `Bundle.registry` and instantiated via `Bundle.create_bundles!`. Prevents V8 isolate creation before fork.
## [0.1.0-alpha.5] - 2026-05-04

### Added
- `Bundle#render_chunks` — chunked render that yields HTML fragments incrementally as they arrive from JS. Returns an `Enumerator` when no block is given (Rack 3 compatible as response body); yields each chunk to the block when one IS given. JS bundles push chunks via `globalThis.__ssr_push_chunk(string)`. Error and timeout semantics match `render`.
- V8 termination watchdog — a dedicated OS thread per render that calls `terminate_execution()` when the render timeout expires. Enables timeout and OOM detection for synchronous blocking JS (e.g., infinite `while` loops). Previously, only async renders (Promises) respected the timeout.
- Branch coverage enforcement in `coverage:check` task — computes merged branch coverage from raw `.resultset.json` (works around SimpleCov 0.22 merger limitation).

### Changed
- **BREAKING:** `Bundle#render_stream_chunks` renamed to `Bundle#render_chunks`. Internal JS globals renamed from `__ssr_stream_*` to `__SSR_DENO_*` and the sentinel from `__SSR_STREAM_SENTINEL` to `__SSR_DENO_SENTINEL`.
- **BREAKING:** `Bundle#render_stream` removed — use `Bundle#render` (always runs the event loop now).
- **BREAKING:** `render(event_loop:)` keyword argument removed — the event loop is always active. Macrotasks, timers, and Promises fire during every render.
- `native_render` now uses the event-loop path internally (was direct V8 function call). Async renders (Promises) resolve naturally; sync renders complete on first poll tick.
- Render timeout is now enforced by the watchdog thread (sole authority). The previous inline `Instant::now() >= deadline` check has been removed — eliminates race conditions between two timeout mechanisms.
- Bundle identifiers now use `<basename>#<object_id>` format (e.g. `entry-server.js#47278032594620`) instead of bare `object_id`. Improves readability in instrumentation events, error messages, and logs.

### Fixed
- Render now correctly raises `SSR::Deno::RenderError` when the JS render function returns a rejected Promise. Previously, rejections were silently returned as a successful result string.

## [0.1.0-alpha.4] - 2026-05-04

### Added
- `Bundle#render` now accepts `event_loop: true` to run the V8 event loop during rendering. This enables macrotask-based APIs (`setTimeout`, `MessagePort`) to fire during SSR, and is a prerequisite for React 19 streaming SSR. `Bundle#render_stream` is available as an alias. Adds event loop integration via `MainWorker::run_up_to_duration` and the `op_ssr_push_chunk` op.
- V8 OOM protection: `near_heap_limit_callback` + `terminate_execution` prevents fatal process crash when a user SSR component exceeds `max_heap_size_mb`. V8 OOM now raises `SSR::Deno::JsRuntimeOutOfMemoryError` (a dedicated exception class, sibling of `RenderError`).
- Stability tests: leak detection (heap growth < 3x over 100 renders), large payload, edge-case data, rapid reload, OOM produces `JsRuntimeOutOfMemoryError`.
- Env var-based config for `SSR::Deno` settings (4 native settings) via `SSR_DENO_` prefix. Env vars act as defaults; setters override. Added getter methods (`max_heap_size_mb`, `isolate_pool_size`, `render_timeout_ms`, `node_builtins_enabled?`).
- New sample: `samples/node-ssr-app` — vanilla TypeScript SSR with esbuild, zero Deno. Node.js build (`npm run build`) and serve (`node serve.mjs`).
- New sample: `samples/vite-preact-ssr-app` — Preact SSR with Vite, uses `resolve.alias` for React compat.
- New sample: `samples/webpack-ssr-app` — vanilla TypeScript SSR with Webpack 5, no framework.
- New sample: `samples/webpack-react-ssr-app` — React 19 SSR with Webpack 5.
- `SSR::Deno.heap_stats!` — raises `JsRuntimeNotInitializedError` / `JsRuntimeWorkerError` instead of returning empty Hash.

### Changed
- `setup_require` is now idempotent — skips the async import + microtask poll loop when `globalThis.require` is already set from a prior bundle load into the same isolate. Saves ~10ms per subsequent bundle load with `node_builtins: true`.
- `SSR::Deno.heap_stats` now returns empty Hash with warning instead of raising when runtime not initialized. Use `heap_stats!` to get the old behavior.
- README rewritten from scratch: self-contained quick start (`File.write` inline bundle), no inline framework examples (links to samples instead), expandable samples table with clickable directory links.
- All Vite-based sample directories prefixed with `vite-`: `vanilla-ssr-app` → `vite-ssr-app`, `react-ssr-app` → `vite-react-ssr-app`, `vue-ssr-app` → `vite-vue-ssr-app`, `svelte-ssr-app` → `vite-svelte-ssr-app`, `preact-ssr-app` → `vite-preact-ssr-app`, `react-mui-ssr-app` → `vite-react-mui-ssr-app`, `react-mui-emotion-ssr-app` → `vite-react-mui-emotion-ssr-app`, `react-emotion-mui-dashboard-ssr-app` → `vite-react-emotion-mui-dashboard-ssr-app`.
- barebone sample now has standalone `serve.deno.ts` HTTP server (consistent with all others).
- Dashboard render timeout increased to 2000ms to prevent flaky CI timeouts.
- Async render polling: replace fixed 10,000 iteration count with configurable timeout-based deadline. Add 100µs sleep between polls to reduce CPU usage. Outer `recv_timeout` now has 100ms buffer to serve as V8-stuck safety net while inner deadline handles normal async timeouts.
- `setup_require` poll loop now uses time-based deadline (10ms) + 100µs sleep, matching the `call_render` pattern. Added post-poll verification to detect `createRequire` failure early — raises `BundleLoad` error at bundle load time instead of failing later with confusing "require is not defined" errors.

## [0.1.0-alpha.3] - 2026-05-02

### Added
- New sample: `samples/deno-native-ssr-app` — vanilla SSR with Deno's built-in `Deno.serve()`, no Vite, no build step.
- New sample: `samples/deno-native-react-ssr-app` — React 19 SSR with Deno native `npm:` imports, no Vite, no build step.
- New sample: `samples/barebone-ssr-app` — plain JS SSR bundle (no framework, no Deno APIs), loadable directly via `SSR::Deno::Bundle`.
- V8 heap metrics via `SSR::Deno.heap_stats` — returns `total_heap_size`, `used_heap_size`, `heap_size_limit`, and 10 other V8 memory counters as a Hash. Rails subscriber emits `heap_stats.ssr_deno` every N renders (configurable via `config.ssr_deno.heap_stats_sample_rate`, default 100).
- Async SSR render support — `call_render` detects `v8::Promise` return and polls V8 microtask queue until settlement. Enables Vue 3 SSR and other async render frameworks.
- New sample: `samples/vite-svelte-ssr-app` — Svelte 5 SSR with `@sveltejs/vite-plugin-svelte`.
- New sample: `samples/vite-react-mui-ssr-app` — React 19 + MUI v9 SSR (plain HTML, no CSS extraction).
- New sample: `samples/vite-react-mui-emotion-ssr-app` — React 19 + MUI v9 SSR with Emotion CSS extraction.
- New sample: `samples/vite-react-emotion-mui-dashboard-ssr-app` — full MUI dashboard with charts, data grid, date pickers.
- New sample: `samples/vite-react-ssr-app` — React 19 SSR.
- New sample: `samples/vite-vue-ssr-app` — Vue 3 SSR.
- README: add SSR bundle creation guide (bundle contract, vanilla/Vue/Svelte/React patterns).
- Serve ports renumbered by complexity: barebone=3100, deno-native=3101, vite-ssr=3102, deno-native-react=3103, vite-svelte=3104, vite-vue=3105, vite-preact=3106, vite-react=3107, vite-react-mui=3108, vite-react-mui-emotion=3109, vite-react-emotion-mui-dashboard=3110.
- Vite edge-light resolve conditions — `@emotion/cache` no longer resolves to browser build under `ssr.target: 'webworker'`. Eliminates the need for a `document` stub in MUI SSR samples.
- `SSR::Deno.node_builtins_enabled=` config option (default: `false`) — enables Node.js built-in module support for bundles that call `require()` for `stream`, `buffer`, `events`, etc. Required for `@emotion/server` and similar packages. Adds ~50ms to worker init. Disabled by default.
- `AGENTS.md` renamed from `CLAUDE.md` (OpenCode canonical name).
- Refactored `Rakefile` — task namespaces extracted to `rakelib/` (`cargo.rake`, `samples.rake`, `test.rake`).
- Renamed `test_integration_vite_ssr.rb` to `integration_samples_test.rb`.
- Split test suite: `test:main` (52 tests, no node_builtins) and `test:node_builtins` (1 test, node_builtins enabled). Merged coverage validated at 100%.
- `/.opencode/` added to `.gitignore`.
- Rails config: `node_builtins_enabled` option added to generator template.

## [0.1.0-alpha.2] - 2026-05-02

### Changed
- Rails dev/test: `isolate_pool_size` defaults to 1 (was auto-detect). Most SSR in dev/test is single-request and doesn't benefit from concurrent isolates. Set `config.ssr_deno.isolate_pool_size = nil` in your initializer to restore auto-detect.

### Added
- V8 heap size limit via `SSR::Deno.max_heap_size_mb=` (default: 64 MB) — caps V8 old-generation memory to prevent runaway growth. Configurable in Rails via `config.ssr_deno.max_heap_size_mb`.
- Render timeout — hung SSR renders (infinite loops, runaway recursion) now raise `SSR::Deno::RenderError` after a configurable duration.
- Configurable render timeout via `SSR::Deno.render_timeout_ms=` (default 500ms, range 100–300000ms) — set before pool init.
- Multi-bundle support via `SSR::Deno::Bundle` class with per-bundle IDs
- `SSR::Deno::Bundle::Registry` — thread-safe named bundle storage
- `native_load_bundle(bundle_id, bundle_path)` for dynamic bundle loading
- Rails integration: `Railtie`, `Helper` (`ssr_render`), `InstallGenerator`
- `ActiveSupport::Notifications` instrumentation (`render.ssr_deno`, `bundle_load.ssr_deno`, `bundle_miss.ssr_deno`)
- Typed error hierarchy: `JsRuntimeInitializationError`, `JsRuntimeNotInitializedError`, `JsRuntimeWorkerError`, `BundleNotFoundError`, `RenderError`
- Ractor-safe native extension (`rb_ext_ractor_safe(true)`)
- Ractor concurrency test

### Changed
- Refactored from single `init_runtime`/`render` API to `Bundle.new(path)`/`bundle.render(data)`
- `DenoRuntimeWrapper::new()` no longer takes a bundle path — bundles loaded separately
- `worker_thread_main` now handles both `LoadBundle` and `Render` messages
- `NopPermissionDescriptorParser` replaces `AllowAllPermissionDescriptorParser`

### Security
- Worker now runs with `Permissions::none_without_prompt()` — all Deno permissions denied
- Replaced `FsModuleLoader` with `NoopModuleLoader` — dynamic `import()` rejected at loader level
- Bundle path symlink-escape check: canonical path must remain within original parent directory
- TOCTOU fix in `init_runtime` via double-checked locking with `INIT_LOCK: Mutex<()>`
- Filesystem paths redacted from error messages (filename only)

## [0.1.0-alpha.1] - 2026-04-25

- Initial release
