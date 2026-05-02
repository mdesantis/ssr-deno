## [Unreleased]

### Added
- V8 heap metrics via `SSR::Deno.heap_stats` — returns `total_heap_size`, `used_heap_size`, `heap_size_limit`, and 10 other V8 memory counters as a Hash. Rails subscriber emits `heap_stats.ssr_deno` every N renders (configurable via `config.ssr_deno.heap_stats_sample_rate`, default 100).
- Async SSR render support — `call_render` detects `v8::Promise` return and polls V8 microtask queue until settlement. Enables Vue 3 SSR and other async render frameworks.

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
