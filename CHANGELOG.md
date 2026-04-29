## [Unreleased]

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
