## Unreleased

### Added
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
- Renamed `test_integration_vite_ssr.rb` to `test_integration_samples.rb`.
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
