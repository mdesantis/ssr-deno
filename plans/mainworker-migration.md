# MainWorker Migration Plan (Option C)

## Goal

Migrate [`ext/ssr_deno/src/deno_runtime_wrapper.rs`](../ext/ssr_deno/src/deno_runtime_wrapper.rs) from using `JsRuntime` directly to using `deno_runtime::worker::MainWorker` with minimal bootstrap.

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

## Required Types

### 1. `InNpmPackageChecker` (from `node_resolver` crate)

Trait:
```rust
pub trait InNpmPackageChecker {
    fn in_npm_package(&self, specifier: &Url) -> bool;
}
```

**Plan**: Define a minimal `NopInNpmPackageChecker` in our crate that always returns `false`.

### 2. `NpmPackageFolderResolver` (from `node_resolver` crate)

Trait:
```rust
pub trait NpmPackageFolderResolver {
    fn resolve_package_folder_from_package(...);
    fn resolve_package_folder_from_specifier(...);
}
```

**Plan**: Define a minimal `NopNpmPackageFolderResolver` that returns errors (we don't use npm packages).

### 3. `ExtNodeSys` (from `deno_node` crate, uses `#[sys_traits::auto_impl]`)

This trait is automatically implemented for any type that implements:
- `NodeResolverSys` (from `node_resolver` — requires `FsCanonicalize` + `FsMetadata` + `FsRead` + `FsReadDir` + `FsOpen`)
- `EnvCurrentDir` (from `sys_traits`)
- `Clone`

**Plan**: Use `deno_runtime::deno_fs::RealFs` which implements `FileSystem`, and check if it also satisfies the required traits. If not, define a minimal wrapper.

### 4. `WorkerServiceOptions` fields

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

**Plan**: Use defaults / minimal values for all:
- `blob_store`: `Arc::new(BlobStore::default())`
- `broadcast_channel`: `InMemoryBroadcastChannel::default()`
- `feature_checker`: `Arc::new(FeatureChecker::default())` — need to check if `Default` exists
- `fs`: `Arc::new(deno_fs::RealFs)`
- `module_loader`: `Rc::new(deno_core::FsModuleLoader)`
- `node_services`: `None`
- `npm_process_state_provider`: `None`
- `permissions`: `PermissionsContainer::allow_all(...)` — requires a `PermissionDescriptorParser`
- `root_cert_store_provider`: `None`
- `fetch_dns_resolver`: `Default::default()`
- `shared_array_buffer_store`: `None`
- `compiled_wasm_module_store`: `None`
- `v8_code_cache`: `None`
- `bundle_provider`: `None`

### 5. `WorkerOptions` fields

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

**Plan**: Use `Default::default()` for most fields, with minimal overrides:
- `bootstrap`: `BootstrapOptions::default()` (provides Deno version, user agent, etc.)
- `extensions`: `vec![]` (the `from_options` method adds all standard extensions automatically)
- `create_web_worker_cb`: Need to provide the default callback (unimplemented web workers)
- `stdio`: `Stdio::default()` or `Stdio::inherit()`

## Implementation Steps

### Step 1: Add required dependencies to `Cargo.toml`

We need to check which crates are already transitive dependencies and which need to be added explicitly. The `deno_runtime` crate re-exports most of them.

### Step 2: Define minimal NOP types for generic parameters

Create a small module (or inline types) for:
- `NopInNpmPackageChecker` — always returns `false`
- `NopNpmPackageFolderResolver` — always returns error
- A type that implements `ExtNodeSys` (via `sys_traits::auto_impl`)

### Step 3: Rewrite `DenoRuntimeWrapper`

Replace `JsRuntime` with `MainWorker`:
- Constructor calls `MainWorker::bootstrap_from_options`
- `block_on_render` accesses `worker.js_runtime` (pub field) for V8 operations
- Keep `UnsafeCell` pattern for thread safety

### Step 4: Update `lib.rs` if needed

The magnus bindings should remain unchanged since the `DenoRuntimeWrapper` API surface stays the same.

## Key Risks

1. **`PermissionsContainer::allow_all` requires `Arc<dyn PermissionDescriptorParser>`** — need to provide a minimal parser or find if there's a simpler constructor
2. **`ExtNodeSys` auto-implementation** — may require implementing several filesystem traits on our type
3. **`create_web_worker_cb`** — the default in `WorkerOptions::default()` already provides `unimplemented!("web workers are not supported")`, so this should be fine
4. **Compile-time verification** — the generic types may require trial-and-error to get right

## Verification

After implementation, verify by:
1. Running `./bin/compile` to build the native extension
2. Testing from Ruby console:
   ```ruby
   require 'ssr/deno'
   bundle_path = File.expand_path('samples/vite-ssr-app/dist/server/entry-server.js')
   SSR::Deno.init_runtime(bundle_path)
   result = SSR::Deno.render({component_data: {component_name: "hello_world"}, props: {name: "World"}, url: "/"}.to_json)
   puts result
   ```
