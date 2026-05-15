# Extract Dev-Mode to `ssr_deno_dev_mode` Crate

**Status (2026-05-14)**: ✅ COMPLETED. Commit `3c32c46`.

## Goal

Move dev-mode code (module loader, npm resolver, require loader, worker builder, setup_require) into a separate `ssr_deno_dev_mode` crate under `ext/ssr_deno/crates/`. Prod-only builds skip compiling `deno_ast`, `deno_resolver`, `node_resolver` → faster compile, smaller binary. The `dev-mode` Cargo feature becomes a simple dependency gate: `ssr_deno_dev_mode = { path = "crates/ssr_deno_dev_mode", optional = true }`.

## Naming normalization

All dev-mode types use the `DevMode` prefix consistently. Files that move into the dev crate get `dev_mode_` prefix. Root-crate dev-specific files keep `dev_` (short, already under `deno_runtime_wrapper/`).

Types:
- `DevModuleLoader` → `DevModeModuleLoader`
- `DevMtimeCache` → `DevModeMtimeCache`
- `DevNodeRequireLoader` → `DevModeNodeRequireLoader`
- `DevIsolateHandle` → `DevModeIsolateHandle`
- `DevWorkerMsg` → `DevModeWorkerMsg`
- `DevWorkerHandle` (Ruby) → stays (magnus wraps it)
- `DevModeBundle` (Ruby) — already correct

Dev-crate files:
- `dev_module_loader.rs` → `dev_mode_module_loader.rs`
- `dev_npm_resolver.rs` → `dev_mode_npm_resolver.rs`
- `dev_builder.rs` → `dev_mode_builder.rs`

Root files (keep):
- `deno_runtime_wrapper/dev_handle.rs` (uses `DevModeIsolateHandle`)
- `deno_runtime_wrapper/dev_load.rs` (uses `DevModeModuleLoader`)
- `deno_runtime_wrapper/dev_worker.rs` (uses `DevModeIsolateHandle`)

Constants:
- Cargo feature `dev-mode` stays (Cargo convention uses hyphens)
- FFI `native_dev_*` stays (magnus snake_case convention)
- `SharedAliasMap`, `SharedCjsPaths`, `DevModeMtimeCache` → types rename, idiomatic `Arc` wrappers

## Architecture

```
ext/ssr_deno/
├── Cargo.toml                  # ssr_deno_dev_mode optional dep, dev-mode feature
├── crates/
│   ├── ssr_deno_core/          # unchanged: config, error, source_mapper, heap (no V8)
│   └── ssr_deno_dev_mode/           # NEW
│       ├── Cargo.toml          # deps: deno_runtime, deno_ast, deno_resolver, node_resolver, magnus
│       └── src/
│           ├── lib.rs                  # register_dev_mode_ffi(), public re-exports
│           ├── dev_mode_module_loader.rs
│           ├── dev_mode_npm_resolver.rs
│           ├── require_loader.rs       # DevModeNodeRequireLoader (dev-only)
│           ├── dev_mode_builder.rs
│           └── setup_require.rs
└── src/
    ├── lib.rs                  # magnus init calls register_dev_mode_ffi() if dev-mode
    ├── sys.rs                  # Sys impl (stays; used by both via ssr_deno_sys crate — see Sys crate section)
    ├── require_loader.rs       # SSRDenoNodeRequireLoader stays (prod)
    └── deno_runtime_wrapper/
        ├── mod.rs
        ├── worker.rs           # worker_thread_main (prod), setup_require removed
        ├── builder.rs          # build_worker (prod)
        ├── isolate_pool.rs     # prod pool
        ├── render.rs           # render engine (shared)
        ├── render_chunked.rs   # render engine (shared)
        ├── dev_handle.rs       # DevModeIsolateHandle, DevModeWorkerMsg (uses render + tokio)
        ├── dev_load.rs         # dev_load_entry, warm_cjs_cache (uses MainWorker + dev types)
        └── dev_worker.rs       # dev_worker_thread_main (uses render + dev types)
```

## Boundary

| Layer | Lives in | V8 dep | magnus dep | Notes |
|-------|---------|--------|-----------|-------|
| Pure types (config, error, heap, source_mapper) | `ssr_deno_core` | No | No | Already extracted |
| Dev module loading (loader, resolver, builder, require, setup_require) | `ssr_deno_dev_mode` | Yes | Yes** | |
| Render engine, worker threads, isolate pool, FFI registration | root `ssr_deno` | Yes | Yes | Root links magnus; dev crate links magnus for its FFI |

**`ssr_deno_dev_mode` links magnus directly — it exports `register_dev_mode_ffi(ruby, mod: RModule)` which root calls during its own `#[magnus::init]`. No callback/trait-object indirection needed.

### Magnuss integration

Root `lib.rs`:
```rust
#[cfg(feature = "dev-mode")]
mod dev_ffi {
    use magnus::{function, method, Ruby, RModule, Error, Value};
    pub fn register(ruby: &Ruby, module: RModule) -> Result<(), Error> {
        ssr_deno_dev_mode::register_dev_mode_ffi(ruby, module)
    }
}

#[magnus::init]
fn init(ruby: &Ruby) -> Result<(), Error> {
    let deno_module = ruby.define_module("SSR")?.define_module("Deno")?;
    // ... prod FFI setup ...
    #[cfg(feature = "dev-mode")]
    dev_ffi::register(ruby, deno_module)?;
    Ok(())
}
```

`ssr_deno_dev_mode/src/lib.rs`:
```rust
pub fn register_dev_mode_ffi(ruby: &Ruby, module: magnus::RModule) -> Result<(), Error> {
    module.define_singleton_method("native_dev_worker_new", function!(..., 2))?;
    module.define_singleton_method("native_dev_render", function!(..., 4))?;
    // ... etc
    Ok(())
}

// Re-exports for root crate
pub use module_loader::{DevModeModuleLoader, DevModeMtimeCache, SharedAliasMap, SharedCjsPaths, drain_cjs_paths, set_aliases};
pub use npm_resolver::build_dev_mode_npm_resolver;
pub use require_loader::DevModeNodeRequireLoader;
pub use builder::build_dev_mode_worker;
pub use setup_require::setup_require;
```

### Dependency graph

```
ssr_deno (root) ──depends──▶ ssr_deno_dev_mode (optional, via dev-mode feature)
     │                            │
     ├── ssr_deno_core            ├── deno_runtime, deno_ast
     ├── magnus                   ├── deno_resolver, node_resolver
     └── deno_runtime             └── magnus
```

`ssr_deno_dev_mode` does NOT depend on root. Root depends on `ssr_deno_dev_mode` (one-way). No circularity.

## What moves

| Source | Destination |
|--------|------------|
| `src/dev_module_loader.rs` | `crates/ssr_deno_dev_mode/src/dev_mode_module_loader.rs` |
| `src/dev_npm_resolver.rs` | `crates/ssr_deno_dev_mode/src/dev_mode_npm_resolver.rs` |
| `src/require_loader.rs` (`DevModeNodeRequireLoader` only) | `crates/ssr_deno_dev_mode/src/require_loader.rs` |
| `deno_runtime_wrapper/dev_builder.rs` | `crates/ssr_deno_dev_mode/src/dev_mode_builder.rs` |
| `deno_runtime_wrapper/worker.rs` (`setup_require` only) | `crates/ssr_deno_dev_mode/src/setup_require.rs` |
| `src/cjs_interop_repro_test.rs` (tests, adapt `build_dev_mode_worker`) | `crates/ssr_deno_dev_mode/tests/cjs_interop_repro.rs` |

## What stays (root)

| File | Why |
|------|-----|
| `src/sys.rs` | Extracted to `crates/ssr_deno_sys/` (step 0.5). Both root and dev depend on `ssr_deno_sys`. |
| `src/require_loader.rs` (`SSRDenoNodeRequireLoader`) | Prod-only, no dev deps. |
| `deno_runtime_wrapper/{worker,builder}.rs` (prod parts) | Prod worker + builder. |
| `deno_runtime_wrapper/{render,render_chunked}.rs` | Shared engine. |
| `deno_runtime_wrapper/{isolate_pool,dev_handle,dev_load,dev_worker}.rs` | Use `use ssr_deno_dev_mode::*` for moved types. |
| `src/lib.rs` | Magnuss entry point. Imports `ssr_deno_dev_mode::register_dev_mode_ffi`. |

## Steps

### 1. Create crate skeleton
- `crates/ssr_deno_dev_mode/Cargo.toml` with deps: `deno_runtime`, `deno_ast`, `deno_resolver`, `node_resolver`, `magnus`, `serde_json`, `ssr_deno_sys`
- `crates/ssr_deno_dev_mode/src/lib.rs` with `register_dev_mode_ffi()` stub and re-exports
- Add `ssr_deno_dev_mode` to root `Cargo.toml` `[workspace] members`

### 1.5. Extract `Sys` to `ssr_deno_sys` crate
- Create `crates/ssr_deno_sys/Cargo.toml` with deps: `sys_traits`, `deno_error` (only, no V8)
- Copy `src/sys.rs` → `crates/ssr_deno_sys/src/lib.rs`
- Add `ssr_deno_sys` to root `Cargo.toml` `[workspace] members`
- Root `Cargo.toml`: add `ssr_deno_sys = { path = "crates/ssr_deno_sys" }` dep
- Root files: replace `use crate::sys::Sys` → `use ssr_deno_sys::Sys`
- Dev crate `Cargo.toml`: add `ssr_deno_sys = { path = "../ssr_deno_sys" }` dep
- Verify root compiles before proceeding

### 2. Move `dev_module_loader.rs`
- Copy file content to `crates/ssr_deno_dev_mode/src/dev_mode_module_loader.rs`
- Replace `use crate::sys::Sys` → `use ssr_deno_sys::Sys` (imports `Sys` from extracted sys crate — step 0.5)
- Replace `use crate::dev_npm_resolver::*` → `use crate::dev_mode_npm_resolver::*` (refers to sibling module in dev crate)
- Replace `use crate::dev_npm_resolver::build_dev_npm_resolver` → `use crate::dev_mode_npm_resolver::build_dev_mode_npm_resolver` (sibling module in dev crate)

### 3. Move `dev_npm_resolver.rs`
- Copy to `crates/ssr_deno_dev_mode/src/dev_mode_npm_resolver.rs`
- Replace `use crate::sys::Sys` → `use ssr_deno_sys::Sys`
- Rename exported function to `build_dev_mode_npm_resolver`

### 4. Split `require_loader.rs`
- `DevModeNodeRequireLoader` → `ssr_deno_dev_mode/src/require_loader.rs`
- `SSRDenoNodeRequireLoader` stays in root `src/require_loader.rs`

### 5. Move `dev_builder.rs`
- Copy to `crates/ssr_deno_dev_mode/src/dev_mode_builder.rs`
- Replace `use crate::sys::Sys` → `use ssr_deno_sys::Sys`
- Replace `use crate::dev_mode_module_loader::*` → `use crate::dev_mode_module_loader::*` (same crate)
- Replace `use crate::dev_mode_npm_resolver::build_dev_mode_npm_resolver` → `use crate::dev_mode_npm_resolver::build_dev_mode_npm_resolver` (same crate context)
- Replace `use crate::require_loader::DevModeNodeRequireLoader` → `use crate::require_loader::DevModeNodeRequireLoader` (same crate)
- Rename function to `build_dev_mode_worker`

### 6. Extract `setup_require`
- Copy from `deno_runtime_wrapper/worker.rs` (the `pub(crate) fn setup_require(...)`) to `crates/ssr_deno_dev_mode/src/setup_require.rs`
- Remove from root's `worker.rs`; add `use ssr_deno_dev_mode::setup_require` in root's `dev_mode_worker.rs`

### 7. Move test file
- `src/cjs_interop_repro_test.rs` → `crates/ssr_deno_dev_mode/tests/cjs_interop_repro.rs`
- Update imports: `use crate::dev_mode_module_loader::*` → `use ssr_deno_dev_mode::*`
- Update `build_worker` calls → `build_dev_mode_worker`

### 8. Remove dev modules from root `lib.rs`
- Delete `mod dev_module_loader;` and `mod dev_npm_resolver;` (moved to dev crate)
- The `#[cfg(feature = "dev-mode")]` blocks that import and register dev FFI become thin wrappers

### 9. Wire root `Cargo.toml`
```toml
[features]
default = ["dev-mode"]
dev-mode = ["ssr_deno_dev_mode"]

[dependencies]
ssr_deno_dev_mode = { path = "crates/ssr_deno_dev_mode", optional = true }
```

### 10. Wire root `lib.rs`
- Remove `#[cfg(feature = "dev-mode")]` gated blocks (most moved to dev crate)
- Keep FFI registration via `ssr_deno_dev_mode::register_dev_mode_ffi()`
- Root `deno_runtime_wrapper/` files that used `crate::dev_module_loader::*` → `use ssr_deno_dev_mode::*`

### 11. Update root files that reference moved items
- `dev_handle.rs`: `use crate::dev_module_loader::DevModeMtimeCache` → `use ssr_deno_dev_mode::DevModeMtimeCache`
- `dev_load.rs`: `use crate::dev_module_loader::*` → `use ssr_deno_dev_mode::*`
- `dev_worker.rs`: `use crate::dev_builder::build_dev_worker` → `use ssr_deno_dev_mode::build_dev_mode_worker`
- `dev_worker.rs`: `use crate::worker::setup_require` → `use ssr_deno_dev_mode::setup_require`
- `dev_builder.rs` reference in root's `dev_worker.rs` → becomes `ssr_deno_dev_mode::build_dev_mode_worker`
- `lib.rs` tests referencing `dev_module_loader` → test inside dev crate

### 12. Run `bundle exec rake` (default features = dev-mode on)

### 13. ✅ Run `cargo check --no-default-features` (prod-only, no dev deps compiled)

Passed after copying gn-generated V8 binding:
```
cp ext/ssr_deno/target/debug/gn_out/src_binding.rs \
   vendor/rusty_v8/gen/src_binding_simdutf_release_x86_64-unknown-linux-gnu.rs
```
The vendored V8 build script expects the binding at `vendor/rusty_v8/gen/` but the
gn build from source puts it in the cargo `target/` dir. Pre-existing mismatch,
not caused by crate extraction.

### 14. Update `plans/dev-mode-followups.md` — remove this item

## Risk

- **Compile time regression**: `ssr_deno_dev_mode` adds a new crate boundary — might increase incremental compile time slightly for dev-mode builds (extra crate to check). But dev-mode is already the heavy path.
- **Break CI for no-default-features**: CI must test both `default` and `--no-default-features`. The `--no-default-features` path must compile without `deno_ast`/`deno_resolver`/`node_resolver`. This is the whole point — verify it early.
- **`ssr_deno_sys` extraction**: touching `Sys` affects both prod and dev. Must verify root compiles with `ssr_deno_sys` import before proceeding to step 2. Run `bundle exec rake compile` after step 1.5.
- **Workspace members**: root `Cargo.toml` `[workspace]` must list all three sub-crates (`ssr_deno_core`, `ssr_deno_dev_mode`, `ssr_deno_sys`). Missing members cause `cargo` to not compile them.

## Non-goals

- Moving `render.rs` / `render_chunked.rs` to a shared crate — they use `MainWorker`, `deno_runtime`, deeply V8-coupled. No compile-time win.
- Moving `dev_handle.rs` / `dev_load.rs` / `dev_worker.rs` to dev crate — they depend on the render engine in root. Would create a circular dep or require the render engine to move too.
- Renaming FFI methods or Ruby classes — `native_dev_*`, `DevModeBundle`, `DevWorkerHandle` stay as-is.
