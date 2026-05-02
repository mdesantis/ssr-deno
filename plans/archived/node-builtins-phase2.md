# Phase 2 — Custom Module Loader for `node:` Scheme

Replace `NoopModuleLoader` with a loader that allows `import('node:module')`
while rejecting all other module loading (preserving security).

---

## Current state

- **`NoopModuleLoader`** (`deno_core::modules::loaders.rs:192`):
  - `resolve`: delegates to `resolve_import` (standard URL resolution)
  - `load`: always returns `Err("Module loading is not supported.")`
- Only used in `build_worker` at `mod.rs:375`
- Blocks `import('node:module')` → crash/abort

---

## Approach

Create a new struct `NodeBuiltinOnlyModuleLoader` that allows `node:`
scheme URLs through the loader while rejecting everything else. Rename
`nop_types.rs` since it won't contain only no-op types anymore.

### Step 1 — Define the struct

Add to a new file `ext/ssr_deno/src/module_loader.rs` (or extend `nop_types.rs`):

```rust
use std::rc::Rc;
use deno_core::{ModuleLoader, ModuleSpecifier, ModuleLoadResponse,
                ModuleLoadOptions, ModuleLoadReferrer, ResolutionKind,
                ModuleLoaderError, JsErrorBox};

#[derive(Debug, Clone)]
pub struct NodeBuiltinOnlyModuleLoader;
```

### Step 2 — Implement `ModuleLoader::resolve`

For `node:` specifiers, return the specifier as a valid `ModuleSpecifier`.
For all others, reject:

```rust
fn resolve(&self, specifier: &str, referrer: &str, _kind: ResolutionKind)
    -> Result<ModuleSpecifier, ModuleLoaderError>
{
    if specifier.starts_with("node:") {
        return ModuleSpecifier::parse(specifier)
            .map_err(|e| ModuleLoaderError::from(JsErrorBox::from_err(e)));
    }
    Err(ModuleLoaderError::from(JsErrorBox::generic(
        "Only node: scheme modules are supported",
    )))
}
```

### Step 3 — Implement `ModuleLoader::load`

Three sub-options for how to provide the module source code:

**A — Enable Node.js services and extensions**  
The cleanest approach. `WorkerOptions` has:
- `extensions: vec![]` — can add `deno_node` extension
- `node_services: None` — needs to be configured

If we add the proper Node.js extension, `node:module` would be loaded
from Deno's built-in module registry automatically.

**B — Create a Rust-side V8 function**  
Instead of `import('node:module')`, create a V8 function from Rust that
implements a minimal `require` using Deno's internal ops to load Node.js
builtins. This bypasses the module loader entirely.

**C — Pre-register during worker init**  
Before the message loop starts, use `worker.js_runtime.execute_script()`
to evaluate the require setup code through a mechanism that doesn't
require the module loader.

### Step 4 — Use in `build_worker`

Replace:
```rust
module_loader: std::rc::Rc::new(deno_runtime::deno_core::NoopModuleLoader),
```
With:
```rust
module_loader: std::rc::Rc::new(NodeBuiltinOnlyModuleLoader),
```

### Step 5 — Rename `nop_types.rs`

Since the file will no longer contain only no-op types, rename to
`support_types.rs` to reflect its broader purpose. Update all `mod`
declarations and `use` statements accordingly.

---

## Investigation needed before implementation

1. What extensions does `deno_runtime` register by default in
   `bootstrap_from_options`?
2. Is there a `deno_node` extension available in `deno_runtime` v0.255.0?
3. Can we add extensions via `WorkerOptions::extensions`?
4. What does `node_services: None` mean — is there a valid non-None
   configuration that enables Node.js builtins?
5. If the above don't work, can we access Node.js builtins through
   `deno_core`'s internal ops directly?

---

## Verification

1. `bundle exec rake compile` — must compile without errors
2. `deno task build` in `react-mui-emotion-ssr-app` — bundle builds
3. Switch `entry-server.ts` back to `@emotion/server` imports
4. `bundle exec rake test` — `TestIntegrationReactMuiEmotionSSR` passes
5. `deno task serve` — manual test also works
