# MainWorker Migration Plan (Option C) — ✅ Completed

## Goal

Migrate [`ext/ssr_deno/src/deno_runtime_wrapper.rs`](../ext/ssr_deno/src/deno_runtime_wrapper.rs) from using `JsRuntime` directly to using `deno_runtime::worker::MainWorker` with minimal bootstrap.

**Status: ✅ Complete** — migrated, refactored into separate modules, compiled, tested, and committed.

## Why MainWorker?

- Future-proof: access to all Deno Web APIs (fetch, WebSocket, crypto, etc.) as needs grow
- `MainWorker.js_runtime` is a `pub` field, so we can still do direct V8 operations
- Proper Deno lifecycle (bootstrap, load events, etc.) if needed later

## Key API: `MainWorker::bootstrap_from_options`

```rust
pub fn bootstrap_from_options<
    TInNpmPackageChecker: InNpmPackageChecker + 'static,
    TNpmPackageFolderResolver: NpmPackageFolderResolver + 'static,
    TExtNodeSys: ExtNodeSys + 'static,
>(
    main_module: &ModuleSpecifier,
    services: WorkerServiceOptions<TInNpmPackageChecker, TNpmPackageFolderResolver, TExtNodeSys>,
    options: WorkerOptions,
) -> Self
```

## Required Types — Implemented

### 1. `InNpmPackageChecker` (from `node_resolver` crate)

Trait:
```rust
pub trait InNpmPackageChecker {
    fn in_npm_package(&self, specifier: &Url) -> bool;
}
```

**Implementation**: [`NopInNpmPackageChecker`](../ext/ssr_deno/src/nop_types.rs) — always returns `false`.

### 2. `NpmPackageFolderResolver` (from `node_resolver` crate)

Trait:
```rust
pub trait NpmPackageFolderResolver {
    fn resolve_package_folder_from_package(...);
    fn resolve_package_folder_from_specifier(...);
}
```

**Implementation**: [`NopNpmPackageFolderResolver`](../ext/ssr_deno/src/nop_types.rs) — returns `PackageFolderResolveErrorKind::PackageNotFound` for all methods.

### 3. `ExtNodeSys` (from `deno_node` crate, uses `#[sys_traits::auto_impl]`)

This trait is automatically implemented for any type that implements:
- `NodeResolverSys` (from `node_resolver` — requires `FsCanonicalize` + `FsMetadata` + `FsRead` + `FsReadDir` + `FsOpen`)
- `EnvCurrentDir` (from `sys_traits`)
- `Clone`

**Implementation**: [`Sys`](../ext/ssr_deno/src/sys.rs) — a custom type implementing all required `sys_traits` traits, delegating to real filesystem/environment operations. Includes wrapper types `RealMetadata`, `RealDirEntry`, `RealFile` for the trait object requirements.

### 4. `PermissionDescriptorParser` (from `deno_permissions` crate)

Required by `PermissionsContainer::allow_all(Arc<dyn PermissionDescriptorParser>)`.

**Implementation**: [`AllowAllPermissionDescriptorParser`](../ext/ssr_deno/src/nop_types.rs) — minimal parser implementing all ~14 trait methods with `unreachable!()` bodies (since permissions are allow-all, these are never called).

### 5. `WorkerServiceOptions` fields

```rust
pub struct WorkerServiceOptions<...> {
    pub blob_store: Arc<BlobStore>,
    pub broadcast_channel: InMemoryBroadcastChannel,
    pub deno_rt_native_addon_loader: Option<DenoRtNativeAddonLoaderRc>,
    pub feature_checker: Arc<FeatureChecker>,
    pub fs: Arc<dyn FileSystem>,
    pub module_loader: Rc<dyn ModuleLoader>,
    pub node_services: Option<NodeExtInitServices<...>>,
    pub npm_process_state_provider: Option<NpmProcessStateProviderRc>,
    pub permissions: PermissionsContainer,
    pub root_cert_store_provider: Option<Arc<dyn RootCertStoreProvider>>,
    pub fetch_dns_resolver: Resolver,
    pub shared_array_buffer_store: Option<SharedArrayBufferStore>,
    pub compiled_wasm_module_store: Option<CompiledWasmModuleStore>,
    pub v8_code_cache: Option<Arc<dyn CodeCache>>,
    pub bundle_provider: Option<Arc<dyn BundleProvider>>,
}
```

**Implementation**: All fields use defaults/minimal values:
- `blob_store`: `Arc::new(BlobStore::default())`
- `broadcast_channel`: `InMemoryBroadcastChannel::default()`
- `feature_checker`: `Arc::new(FeatureChecker::default())`
- `fs`: `Arc::new(deno_fs::RealFs)`
- `module_loader`: `Rc::new(deno_core::FsModuleLoader)`
- `node_services`: `None`
- `npm_process_state_provider`: `None`
- `permissions`: `PermissionsContainer::allow_all(Arc::new(AllowAllPermissionDescriptorParser))`
- `root_cert_store_provider`: `None`
- `fetch_dns_resolver`: `Default::default()`
- `shared_array_buffer_store`: `None`
- `compiled_wasm_module_store`: `None`
- `v8_code_cache`: `None`
- `bundle_provider`: `None`

### 6. `WorkerOptions` fields

```rust
pub struct WorkerOptions {
    pub bootstrap: BootstrapOptions,
    pub extensions: Vec<Extension>,
    pub startup_snapshot: Option<&'static [u8]>,
    pub skip_op_registration: bool,
    pub create_params: Option<v8::CreateParams>,
    pub unsafely_ignore_certificate_errors: Option<Vec<String>>,
    pub seed: Option<u64>,
    pub create_web_worker_cb: Arc<ops::worker_host::CreateWebWorkerCb>,
    pub format_js_error_fn: Option<Arc<FormatJsErrorFn>>,
    pub should_break_on_first_statement: bool,
    pub should_wait_for_inspector_session: bool,
    pub trace_ops: Option<Vec<String>>,
    pub cache_storage_dir: Option<PathBuf>,
    pub origin_storage_dir: Option<PathBuf>,
    pub stdio: Stdio,
    pub enable_raw_imports: bool,
    pub enable_stack_trace_arg_in_ops: bool,
    pub unconfigured_runtime: Option<UnconfiguredRuntime>,
}
```

**Implementation**: `Default::default()` with minimal overrides:
- `bootstrap`: `BootstrapOptions::default()`
- `extensions`: `vec![]` (standard extensions added automatically by `from_options`)
- `create_web_worker_cb`: `Arc::new(|_| unimplemented!("web workers are not supported"))`
- `stdio`: `Default::default()`
- All other fields use their default values

## File Structure After Migration

```
ext/ssr_deno/src/
├── lib.rs                     # magnus entrypoint (unchanged API)
├── deno_runtime_wrapper.rs    # DenoRuntimeWrapper only (MainWorker-based)
├── sys.rs                     # Sys type + sys_traits implementations
└── nop_types.rs               # NopInNpmPackageChecker, NopNpmPackageFolderResolver,
                               # AllowAllPermissionDescriptorParser
```

## Key Technical Details

### V8 Scope API Pattern

The `MainWorker` migration required a specific V8 scope access pattern:

```rust
let scope_storage = std::pin::pin!(v8::HandleScope::new(isolate));
let mut scope = scope_storage.init();
let context_local = v8::Local::new(&mut scope, context);
let mut context_scope = v8::ContextScope::new(&mut scope, context_local);
let global = context_local.global(&mut context_scope);
```

- `HandleScope::new(isolate)` returns `ScopeStorage<HandleScope<'_>>`
- `.init()` transitions to `PinnedRef<'_, HandleScope<'_>>`
- `ContextScope::new(&mut scope, context_local)` enters the context
- `context_local.global(&mut context_scope)` gets the global object

### Dependencies Added to `Cargo.toml`

```toml
deno_semver = "=0.9.1"       # Version type for NpmPackageFolderResolver
node_resolver = "=0.84.0"    # InNpmPackageChecker, NpmPackageFolderResolver traits
sys_traits = "=0.1.27"       # FsCanonicalize, FsMetadata, etc. for ExtNodeSys
libc = "0.2"                 # FsFileAsRaw on Unix
```

### Compilation Challenges

1. **37 errors → 15 errors → 3 errors → 0 errors**: Iterative fixes for trait implementations, API mismatches, and V8 scope types
2. **`PermissionDescriptorParser`**: Required implementing ~14 methods with specific return types (`ReadDescriptor`, `WriteDescriptor`, `AllowRunDescriptorParseResult`, etc.)
3. **`FsFile` trait**: Required implementing 11 sub-traits (`Read + Write + Seek + FsFileIsTerminal + FsFileLock + FsFileMetadata + FsFileSetPermissions + FsFileSetTimes + FsFileSetLen + FsFileSyncAll + FsFileSyncData + FsFileAsRaw`)
4. **`WhichSys` trait**: Required `EnvHomeDir + EnvCurrentDir + EnvVar + FsReadDir + FsMetadata + Clone + 'static`

## Verification

- ✅ `./bin/compile` — builds successfully with 0 warnings
- ✅ `bundle exec ruby -e "require 'ssr/deno'; puts SSR::Deno.native_version"` — native extension loads
- ✅ `bundle exec rake test` — all tests pass
