## [Unreleased]

### Changed
- Rails dev/test: `isolate_pool_size` defaults to 1 (was auto-detect). Most SSR in dev/test is single-request and doesn't benefit from concurrent isolates. Set `config.ssr_deno.isolate_pool_size = nil` in your initializer to restore auto-detect.

### Added
- V8 heap size limit via `SSR::Deno.max_heap_size_mb=` (default: 64 MB) — caps V8 old-generation memory to prevent runaway growth. Configurable in Rails via `config.ssr_deno.max_heap_size_mb`. See [`plans/v8-heap-limit.md`](plans/v8-heap-limit.md).
- Render timeout (10s) — hung SSR renders (infinite loops, runaway recursion) now raise `SSR::Deno::RenderError` after 10s instead of blocking the worker thread indefinitely. See [`plans/render-timeout.md`](plans/render-timeout.md).
- Configurable render timeout via `SSR::Deno.render_timeout_ms=` (default 500ms, range 100–300000ms) — set before pool init. See [`plans/configurable-render-timeout.md`](plans/configurable-render-timeout.md).

## [0.1.0-alpha.2] - 2026-04-29

### Added
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
