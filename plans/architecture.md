# SSR-Deno Architecture Plan

## Overview

A Ruby gem that embeds the [`deno_runtime`](https://docs.rs/deno_runtime/latest/deno_runtime/) Rust crate via a native extension to provide server-side rendering (SSR) of Vite-built web applications. The gem loads a Vite SSR production bundle (built with `ssr.target: "webworker"`) and executes it within an embedded V8 isolate with full Deno Web API support, passing JSON data from Ruby and receiving rendered HTML back.

## Architecture

```
  ┌─────────────────────────────────────────────────────────────────────┐
  │                         Ruby Process                                │
  │                                                                     │
  │   ┌──────────┐     ┌─────────────────────┐     ┌────────────────┐   │
  │   │          │     │                     │     │                │   │
  │   │ Ruby App │────>│ SSR::Deno.render    │────>│ Ruby Native    │   │
  │   │          │     │ data                │     │ Extension      │   │
  │   │          │<────│                     │<────│                │   │
  │   └──────────┘     └─────────────────────┘     └───────┬────────┘   │
  │                                                        │            │
  │                                                   JSON │            │
  │                                                        │            │
  │                                              ┌─────────▼────────┐   │
  │                                              │                  │   │
  │                                              │ deno_runtime::   │   │
  │                                              │ MainWorker       │   │
  │                                              │                  │   │
  │                                              └──┬────┬────┬─────┘   │
  │                                                 │    │    │         │
  │                    ┌────────────────────────────┘    │    └──────┐  │
  │                    │                                 │           │  │
  │           ┌────────▼────────┐              ┌────────▼────────┐   │  │
  │           │                 │              │                 │   │  │
  │           │ deno_web        │              │ Self-Contained  │   │  │
  │           │ extension       │              │ Vite SSR Bundle │   │  │
  │           │                 │              │                 │   │  │
  │           └────────┬────────┘              └────────▲────────┘   │  │
  │                    │                                │            │  │
  │           ┌────────▼────────────────────────┐   HTML│            │  │
  │           │                                 │ string│            │  │
  │           │ MessageChannel, setTimeout,     │───────┘            │  │
  │           │ performance, console            │                    │  │
  │           │                                 │                    │  │
  │           └─────────────────────────────────┘                    │  │
  │                                                                  │  │
  │  ┌────────────────────────────────────────────────────────────┐  │  │
  │  │                      Build Time                            │  │  │
  │  │                                                            │  │  │
  │  │   ┌──────────────────────┐     ┌────────────────────────┐  │  │  │
  │  │   │                      │     │                        │  │  │  │
  │  │   │ Vite + React/Vue/    │────>│ dist/server/entry-     │──┼──┘  │
  │  │   │ Svelte               │     │ server.js              │  │     │
  │  │   │                      │     │                        │  │     │
  │  │   └──────────────────────┘     └────────────────────────┘  │     │
  │  │                                                            │     │
  │  └────────────────────────────────────────────────────────────┘     │
  └─────────────────────────────────────────────────────────────────────┘
```

## Data Flow

```
  ┌──────┐         ┌──────────┐         ┌──────────────┐         ┌──────────────────┐
  │ Ruby │         │  magnus  │         │ deno_runtime │         │  Vite SSR Bundle │
  │ App  │         │Extension │         │              │         │                  │
  └──┬───┘         └───┬──────┘         └──────┬───────┘         └────────┬─────────┘
     │                 │                       │                          │
     │ SSR::Deno.render│                       │                          │
     │ ({component_data│                       │                          │
     │  , props})      │                       │                          │
     │────────────────>│                       │                          │
     │                 │                       │                          │
     │                 │ Serialize args to JSON│                          │
     │                 │──┐                    │                          │
     │                 │  │                    │                          │
     │                 │<─┘                    │                          │
     │                 │                       │                          │
     │                 │ Execute JS entry      │                          │
     │                 │ with JSON             │                          │
     │                 │──────────────────────>│                          │
     │                 │                       │                          │
     │                 │                       │ Call render(JSON.parse(  │
     │                 │                       │ args))                   │
     │                 │                       │─────────────────────────>│
     │                 │                       │                          │
     │                 │                       │    Render component      │
     │                 │                       │    to HTML string        │
     │                 │                       │──┐                       │
     │                 │                       │  │                       │
     │                 │                       │<─┘                       │
     │                 │                       │                          │
     │                 │                       │ Return HTML string       │
     │                 │                       │<─────────────────────────│
     │                 │                       │                          │
     │                 │ Return HTML as        │                          │
     │                 │ Rust String           │                          │
     │                 │<──────────────────────│                          │
     │                 │                       │                          │
     │ Return HTML as  │                       │                          │
     │ Ruby String     │                       │                          │
     │<────────────────│                       │                          │
  ┌──┴───┐         ┌───┴──────┐         ┌──────┴───────┐         ┌────────┴─────────┐
  │ Ruby │         │  magnus  │         │ deno_runtime │         │  Vite SSR Bundle │
  │ App  │         │Extension │         │              │         │                  │
  └──────┘         └──────────┘         └──────────────┘         └──────────────────┘
```

## Component Architecture

```
  ┌─────────────────────────────────────────────────────────────────────────────┐
  │                              Ruby Layer                                     │
  │                                                                             │
  │   ┌──────────────────────┐     ┌──────────────┐     ┌──────────────────┐    │
  │   │ SSR::Deno.render     │────>│ JSON.generate│────>│ native_render    │    │
  │   │ Hash                 │     │              │     │                  │    │
  │   └──────────┬───────────┘     └──────────────┘     └──────────────────┘    │
  │              │                                                              │
  └──────────────┼──────────────────────────────────────────────────────────────┘
                 │ magnus bindings
  ┌──────────────┼──────────────────────────────────────────────────────────────┐
  │              ▼              Rust Native Extension (ext/ssr_deno/)           │
  │   ┌──────────────────────┐                                                  │
  │   │ lib.rs               │────┐                                             │
  │   │ (magnus entrypoint)  │    │                                             │
  │   └──────────────────────┘    │                                             │
  │                               ▼                                             │
  │   ┌──────────────────────────────────────────────────────────────────────┐  │
  │   │     deno_runtime_wrapper/  (mod.rs + call_render.rs)                  │  │
  │   └──┬───────────────────────────────┬───────────────────────────────────┘  │
  │      │                               │                                      │
  │      ▼                               ▼                                      │
  │   ┌──────────────────────┐   ┌──────────────────────┐                       │
  │   │ sys.rs               │   │ nop_types.rs         │                       │
  │   │ (Sys type for        │   │ (NOP types for       │                       │
  │   │  sys_traits)         │   │  generics)           │                       │
  │   └──────────────────────┘   └──────────────────────┘                       │
  └──────────────┬──────────────────────────────────────────────────────────────┘
                 │
  ┌──────────────┼──────────────────────────────────────────────────────────────┐
  │              ▼                    deno_runtime Crate                        │
  │   ┌──────────────────────────────────────────────────────────────────────┐  │
  │   │              MainWorker (via bootstrap_from_options)                 │  │
  │   └──┬───────────────────────────────┬───────────────────────────────────┘  │
  │      │                               │                                      │
  │      ▼                               ▼                                      │
  │   ┌──────────────────────┐   ┌──────────────────────┐                       │
  │   │ deno_web extension   │   │ V8 Engine            │                       │
  │   └──────────┬───────────┘   └──────────────────────┘                       │
  │              ▼                                                              │
  │   ┌──────────────────────────────────────────────────────────────────────┐  │
  │   │ MessageChannel, setTimeout, performance, console                     │  │
  │   └──────────────────────────────────────────────────────────────────────┘  │
  └──────────────┬──────────────────────────────────────────────────────────────┘
                 │
  ┌──────────────┼──────────────────────────────────────────────────────────────┐
  │              ▼                         JS Layer                             │
  │   ┌──────────────────────────────────────────────────────────────────────┐  │
  │   │              Vite SSR Bundle (entry-server.js)                       │  │
  │   └──────────────────────────┬───────────────────────────────────────────┘  │
  │                              ▼                                              │
  │   ┌──────────────────────────────────────────────────────────────────────┐  │
  │   │                    globalThis.render function                        │  │
  │   └──────────────────────────────────────────────────────────────────────┘  │
  └─────────────────────────────────────────────────────────────────────────────┘
```

## Directory Structure

```
ssr-deno/
├── ext/
│   └── ssr_deno/                    # Rust crate (Cargo.toml, src/)
│       └── src/
│           ├── lib.rs               # magnus entrypoint
│           ├── deno_runtime_wrapper/
│           │   ├── mod.rs              # IsolateHandle, IsolatePool, thread
│           │   └── call_render.rs      # call_render, heap_stats
│           ├── sys.rs               # Sys type + sys_traits implementations
│           └── nop_types.rs         # NOP types for generic parameters
├── lib/
│   └── ssr/
│       └── deno/                    # Ruby module
│           ├── deno.rb              # Core entry point
│           ├── version.rb           # VERSION constant
│           ├── bundle.rb            # Bundle class (multi-bundle support)
│           ├── bundle/
│           │   └── registry.rb      # Thread-safe Bundle::Registry
│           ├── rails.rb             # Rails integration entry point (opt-in)
│           └── rails/
│               ├── railtie.rb       # Railtie (config, init bundles)
│               ├── helper.rb        # View helper (ssr_render)
│               └── generators/
│                   └── ssr/deno/
│                       ├── install_generator.rb
│                       └── templates/
│                           └── ssr_deno.rb
├── sig/                             # RBS type signatures
├── test/                            # Minitest suite
├── samples/
│   ├── vanilla-ssr-app/              # Sample vanilla TS SSR project
│   ├── vue-ssr-app/                  # Sample Vue 3 SSR project
│   ├── svelte-ssr-app/               # Sample Svelte 5 SSR project
│   └── react-ssr-app/                # Sample React 19 SSR project (deno.json, src/, dist/)
├── plans/                           # Architecture and migration plans
│   ├── architecture.md
│   ├── ci-speedup.md
│   ├── memory-performance-analysis.md
│   ├── rails-integration.md
│   ├── rust-unit-tests.md
│   ├── ssr-process-pool.md
│   ├── streaming-ssr.md
│   ├── v8-heap-metrics.md
│   └── v8-tls-issue.md
├── .github/
│   └── workflows/
│       └── ci.yml                   # CI pipeline
├── Gemfile
├── ssr-deno.gemspec
└── Rakefile
```

## Detailed Component Design

### 1. Rust Native Extension (`ext/ssr_deno/`)

#### `Cargo.toml` Dependencies

```toml
[dependencies]
magnus = { version = "0.8", features = ["embed"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["full"] }
deno_runtime = { version = "0.254.0", features = ["transpile", "hmr"] }
deno_semver = "=0.9.1"
node_resolver = "=0.84.0"
sys_traits = "=0.1.27"
libc = "0.2"

[patch.crates-io]
v8 = { path = "../../vendor/rusty_v8" }
```

The `transpile` feature enables TypeScript transpilation for `deno_telemetry`
extension sources. The `hmr` feature swaps `op_snapshot_options` to a
non-panicking `try_take + unwrap_or_default` path. The `[patch.crates-io]`
entry pins `v8` to a local checkout built with the TLS fix from
[`plans/v8-tls-issue.md`](v8-tls-issue.md).

#### `lib.rs` — magnus Entrypoint

- Defines the `SSR::Deno` Ruby module with a full error hierarchy
- Registers `native_load_bundle(bundle_id, bundle_path)` to evaluate a bundle
- Registers `native_render(bundle_id, json_string)` to call a bundle's render function
- Registers `native_version` to return the crate version
- Uses `POOL: OnceLock<IsolatePool>` for the isolate pool
- Double-checked locking via `POOL_INIT_LOCK: Mutex<()>` prevents TOCTOU races
- Config stored in `CONFIG: Mutex<Config>` with `INITIALIZED: OnceLock<()>` guard
  (Mutex allows multiple fields to be set independently; the guard prevents mutation after init)
- Each `IsolateHandle` holds a Tokio runtime + `MainWorker` on its own `deno-worker-{index}` thread

```rust
use magnus::{function, Error, Module, Object, Ruby};
use std::sync::{Mutex, OnceLock};
use crate::deno_runtime_wrapper::IsolatePool;

static POOL: OnceLock<IsolatePool> = OnceLock::new();
static POOL_INIT_LOCK: Mutex<()> = Mutex::new(());
static INITIALIZED: OnceLock<()> = OnceLock::new();
static CONFIG: Mutex<Config> = Mutex::new(Config::default());

#[magnus::init]
fn init(ruby: &Ruby) -> Result<(), Error> {
    let module = ruby.define_module("SSR")?;
    let deno_module = module.define_module("Deno")?;
    // Error hierarchy
    let base_error = deno_module.define_error("Error", ruby.exception_standard_error())?;
    deno_module.define_error("JsRuntimeInitializationError", base_error)?;
    deno_module.define_error("JsRuntimeNotInitializedError", base_error)?;
    deno_module.define_error("JsRuntimeWorkerError", base_error)?;
    deno_module.define_error("BundleNotFoundError", base_error)?;
    deno_module.define_error("RenderError", base_error)?;
    // Methods
    deno_module.define_singleton_method("native_load_bundle", function!(native_load_bundle, 2))?;
    deno_module.define_singleton_method("native_render", function!(native_render, 2))?;
    deno_module.define_singleton_method("native_version", function!(native_version, 0))?;
    Ok(())
}
```

#### `deno_runtime_wrapper/` — Runtime Lifecycle

This is the core module. The Ruby thread holds only an mpsc `Sender`; the
`deno_runtime::MainWorker` lives on a dedicated background thread
(`"deno-worker"`) along with its own `current_thread` Tokio runtime and a
`LocalSet`. Render calls are sent across the channel and the result is
returned via a `oneshot`.

**Why a dedicated worker thread instead of `UnsafeCell` + GVL:**

`MainWorker` is `!Send + !Sync` (it owns a `v8::OwnedIsolate` and a
`!Send` Tokio context). Earlier versions of this code wrapped it in
`UnsafeCell` and forced `Send + Sync` via `unsafe impl`, relying on Ruby's
GVL to serialize access. That is fragile: Ruby may release the GVL during
blocking operations, and any future move to Ractors or a thread pool
breaks the assumption silently. Pinning the worker to one OS thread and
talking to it via channels removes all `unsafe` from the wrapper while
keeping the public API blocking-friendly for Ruby.

**Why `MainWorker` instead of `JsRuntime`:**

The full `deno_runtime::MainWorker` provides all Deno Web API extensions
out of the box — `MessageChannel`, `setTimeout`, `performance.now()`,
`console`, etc. These are required by frontend frameworks like React 19
(whose scheduler uses `MessageChannel` for async task scheduling). Using
`deno_core::JsRuntime` alone would require manually adding each extension
or writing polyfills, effectively reimplementing `deno_runtime`.

`MainWorker::bootstrap_from_options` is the public constructor that:
1. Creates a `JsRuntime` with all standard Deno extensions
2. Bootstraps the runtime (loads built-in JS modules, initializes ops)
3. Returns a ready-to-use `MainWorker`

**Why `current_thread` Tokio + `LocalSet`:**

Deno's Web API extensions (e.g. `MessagePort` used by React 19's
scheduler) call `deno_unsync::spawn_local` internally, which requires a
`LocalSet` to be active. A multi-threaded runtime is unnecessary —
`MainWorker` is single-threaded — and would also conflict with
`deno_unsync`'s assumptions.

```rust
use std::sync::mpsc;
use std::sync::Arc;

use deno_runtime::deno_core::url::Url;
use deno_runtime::deno_core::v8;
use deno_runtime::deno_permissions::PermissionsContainer;
use deno_runtime::worker::MainWorker;
use deno_runtime::worker::WorkerOptions;
use deno_runtime::worker::WorkerServiceOptions;
use deno_runtime::BootstrapOptions;
use deno_runtime::FeatureChecker;

use crate::nop_types::AllowAllPermissionDescriptorParser;
use crate::nop_types::NopInNpmPackageChecker;
use crate::nop_types::NopNpmPackageFolderResolver;
use crate::sys::Sys;

enum WorkerMsg {
    Render {
        args_json: String,
        reply: tokio::sync::oneshot::Sender<Result<String, String>>,
    },
}

pub struct DenoRuntimeWrapper {
    tx: tokio::sync::mpsc::Sender<WorkerMsg>,
}

impl DenoRuntimeWrapper {
    /// Spawns the Deno worker thread and blocks until it is ready. No bundle
    /// is evaluated at this stage — bundles are loaded later via `load_bundle`.
    pub fn new() -> Result<Self, DenoError> {
        let (tx, rx) = tokio::sync::mpsc::channel::<WorkerMsg>(1);
        let (init_tx, init_rx) = mpsc::sync_channel::<Result<(), String>>(1);

        std::thread::Builder::new()
            .name("deno-worker".into())
            .spawn(move || worker_thread_main(rx, init_tx))
            .map_err(|e| DenoError::WorkerInit(format!("Failed to spawn worker thread: {e}")))?;

        init_rx
            .recv()
            .map_err(|_| DenoError::WorkerInit("Deno worker thread exited unexpectedly during init".into()))?
            .map_err(DenoError::WorkerInit)?;

        Ok(Self { tx })
    }

    /// Evaluates a Vite SSR bundle and registers its `render` function under
    /// `globalThis.__ssr_bundles[bundle_id]`. Safe to call for multiple bundles.
    pub fn load_bundle(&self, bundle_id: &str, bundle_path: &str) -> Result<(), DenoError> {
        // Canonicalizes path, checks symlink-escape, reads file, sends to worker
        // ...
    }

    /// Sends a render request to the worker thread and blocks until the result
    /// arrives. Returns the result as a JSON string.
    pub fn block_on_render(&self, bundle_id: &str, args_json: &str) -> Result<String, DenoError> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .blocking_send(WorkerMsg::Render {
                bundle_id: bundle_id.to_string(),
                args_json: args_json.to_string(),
                reply: reply_tx,
            })
            .map_err(|_| DenoError::WorkerDied("Deno worker thread has exited".into()))?;
        reply_rx
            .blocking_recv()
            .map_err(|_| DenoError::WorkerDied("Deno worker thread exited before sending a reply".into()))?
    }
}

fn worker_thread_main(
    mut rx: tokio::sync::mpsc::Receiver<WorkerMsg>,
    init_tx: mpsc::SyncSender<Result<(), String>>,
) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    tokio::task::LocalSet::new().block_on(&rt, async move {
        let main_module_url = Url::parse("https://ssr-deno.local/").unwrap();
        let mut worker = build_worker(&main_module_url).unwrap();
        let _ = init_tx.send(Ok(()));

        while let Some(msg) = rx.recv().await {
            match msg {
                WorkerMsg::LoadBundle { bundle_id, bundle_code, script_name, reply } => {
                    let result = load_bundle_in_worker(&mut worker, &bundle_id, bundle_code, script_name);
                    let _ = reply.send(result);
                }
                WorkerMsg::Render { bundle_id, args_json, reply } => {
                    let result = call_render(&mut worker, &bundle_id, &args_json);
                    let _ = reply.send(result);
                }
            }
        }
    });
}
```

`build_worker` constructs `WorkerServiceOptions` + `WorkerOptions` and
returns a `MainWorker` via `bootstrap_from_options`. `call_render` enters
the V8 `HandleScope` / `ContextScope`, looks up `globalThis.render`, and
invokes it with the JSON string argument. See the source for the full
boilerplate.

#### `sys.rs` — System Type for `ExtNodeSys`

Contains the `Sys` type that implements all `sys_traits` required by `ExtNodeSys` (via `#[sys_traits::auto_impl]`):

- `FsCanonicalize`, `FsMetadata`, `FsRead`, `FsReadDir`, `FsOpen` (for `NodeResolverSys`)
- `EnvCurrentDir`, `EnvHomeDir`, `EnvVar` (for `WhichSys`)
- `Clone + 'static`

Also includes wrapper types:
- `RealMetadata` — wraps `std::fs::Metadata`, implements `FsMetadataValue`
- `RealDirEntry` — wraps `std::fs::DirEntry`, implements `FsDirEntry`
- `RealFile` — wraps `std::fs::File`, implements `FsFile` (with all 11 sub-traits)

#### `nop_types.rs` — NOP Types for Generic Parameters

Contains three types required by `MainWorker::bootstrap_from_options`:

- **`NopInNpmPackageChecker`** — always returns `false` (no npm packages)
- **`NopNpmPackageFolderResolver`** — always returns `PackageFolderResolveErrorKind::PackageNotFound`
- **`AllowAllPermissionDescriptorParser`** — implements `PermissionDescriptorParser` with `unreachable!()` bodies (never called since permissions are allow-all)

### 2. Ruby Layer

#### `SSR::Deno::Bundle` Class

The Ruby API is built around the `Bundle` class, which wraps a single Vite SSR bundle and manages its lifecycle:

```ruby
module SSR
  module Deno
    class Bundle
      class << self
        attr_reader :registry
      end

      @registry = Registry.new

      def initialize(bundle_path)
        @bundle_path = bundle_path.to_s
        @bundle_id = object_id.to_s
        @mtime = File.mtime(@bundle_path)
        @auto_reload = false
        load
      end

      def render(data = nil, raw_input: false, raw_output: false)
        reload_if_changed if @auto_reload
        json_input = raw_input ? data : JSON.generate(data)
        result = SSR::Deno.native_render(@bundle_id, json_input)
        raw_output ? result : JSON.parse(result)
      end
    end
  end
end
```

The native extension registers three methods:
- `native_load_bundle(bundle_id, bundle_path)` — evaluates a bundle and registers its render function under `globalThis.__ssr_bundles[bundle_id]`
- `native_render(bundle_id, json_string)` — calls the JS `render` function for a specific bundle
- `native_version` — returns the crate version

The Ruby `Bundle` class handles JSON serialization, mtime-based auto-reload, and `ActiveSupport::Notifications` instrumentation, keeping the native interface simple and the Ruby API ergonomic.

#### `SSR::Deno::Bundle::Registry`

A thread-safe registry for named bundles, used primarily by the Rails integration:

```ruby
registry = SSR::Deno::Bundle::Registry.new
registry.register(:application, bundle)
registry[:application]  # => bundle
registry.bundle(:application)  # => bundle
```

### 3. Vite SSR Bundle Contract

The Vite project should be configured with:

```ts
// vite.config.ts
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  ssr: {
    target: 'webworker',
    noExternal: true,          // Inline all deps into a single self-contained bundle
  },
  build: {
    ssr: true,
    outDir: 'dist/server',
    rollupOptions: {
      input: 'src/entry-server.ts',
    },
  },
})
```

> **`ssr.noExternal: true`** is critical. Without it, Vite produces a bundle with external `import` statements for dependencies like `react` and `react-dom`. The embedded Deno runtime cannot resolve these external imports — it has no package manager or `node_modules` access. With `noExternal: true`, Vite (via rolldown) inlines **all** dependencies into a single self-contained file (~448KB for React 19) with zero `import` statements. The bundle assigns `render` to `globalThis`, making it ideal for direct evaluation in the embedded V8 isolate.

The entry file should assign a `render` function to `globalThis`:

```ts
// src/entry-server.ts
import { renderToString } from 'react-dom/server'
import { createElement } from 'react'
import App from './App.tsx'

function render(argsJson: string): string {
  const context = JSON.parse(argsJson)
  const html = renderToString(
    createElement(App, {
      data: context.component_data,
      extra: context.props,
    })
  )
  return html
}

// Assign to globalThis for embedded V8 evaluation
globalThis.render = render
```

## Error Handling Strategy

All Rust-side failures (bundle path resolution, V8 evaluation, missing or
non-callable `render`, JS exception, worker thread death) are converted
to a `RuntimeError` at the magnus boundary in
[`ext/ssr_deno/src/lib.rs`](../ext/ssr_deno/src/lib.rs) via
`runtime_error(...)`. The Ruby layer exposes `SSR::Deno::Error` for
callers to rescue. No timeout, retry, or bundle-reload behavior is
implemented yet.

## Implementation Phases

### Phase 1: Project Scaffolding ✅
- Add Rust toolchain setup to the gem
- Create `ext/ssr_deno/` directory with `Cargo.toml`
- Set up `Rakefile` tasks for native extension compilation
- Add `rb-sys` and `magnus` as dependencies
- Create a minimal "hello world" native extension to verify the build pipeline

### Phase 2: Embed `deno_runtime` ✅

**Key Decision**: Use [`deno_runtime`](https://crates.io/crates/deno_runtime) with `MainWorker::bootstrap_from_options` instead of bare `deno_core::JsRuntime`. The full `deno_runtime` provides all Deno Web API extensions (`deno_web`, `deno_webidl`, etc.) that frontend frameworks like React 19 depend on — `MessageChannel`, `setTimeout`, `performance.now()`, `console`, etc. Using `deno_core` alone would require manually adding each extension or writing polyfills, effectively reimplementing `deno_runtime`.

**Completed steps:**

1. ✅ **Updated [`ext/ssr_deno/Cargo.toml`](../ext/ssr_deno/Cargo.toml)**
   - Added `deno_runtime`, `deno_semver`, `node_resolver`, `sys_traits`, `libc`

2. ✅ **Rewrote [`ext/ssr_deno/src/deno_runtime_wrapper/`](../ext/ssr_deno/src/deno_runtime_wrapper/mod.rs)**
   - Uses `MainWorker::bootstrap_from_options` with three generic type parameters
   - V8 scope access via `pin!/init()/ContextScope` pattern
   - `MainWorker` pinned to a dedicated `"deno-worker"` thread with a
     `current_thread` Tokio runtime + `LocalSet`; Ruby thread holds an
     mpsc `Sender` and round-trips render calls via `oneshot`. No
     `unsafe` and no `UnsafeCell` in the wrapper.

3. ✅ **Created [`ext/ssr_deno/src/sys.rs`](../ext/ssr_deno/src/sys.rs)**
   - `Sys` type implementing all `sys_traits` for `ExtNodeSys`
   - Wrapper types: `RealMetadata`, `RealDirEntry`, `RealFile`

4. ✅ **Created [`ext/ssr_deno/src/nop_types.rs`](../ext/ssr_deno/src/nop_types.rs)**
   - `NopInNpmPackageChecker`, `NopNpmPackageFolderResolver`, `NopPermissionDescriptorParser`

5. ✅ **Updated [`ext/ssr_deno/src/lib.rs`](../ext/ssr_deno/src/lib.rs)**
   - Added `mod sys;` and `mod nop_types;` declarations
   - Added `native_version` method

6. ✅ **Refactored into separate modules**
   - [`ext/ssr_deno/src/sys.rs`](../ext/ssr_deno/src/sys.rs) — `Sys` type + all `sys_traits` impls
   - [`ext/ssr_deno/src/nop_types.rs`](../ext/ssr_deno/src/nop_types.rs) — NOP types for generic params
   - [`ext/ssr_deno/src/deno_runtime_wrapper/mod.rs`](../ext/ssr_deno/src/deno_runtime_wrapper/mod.rs) — `IsolateHandle`, `IsolatePool`, worker thread
   - [`ext/ssr_deno/src/deno_runtime_wrapper/call_render.rs`](../ext/ssr_deno/src/deno_runtime_wrapper/call_render.rs) — `call_render`, `collect_heap_stats`

7. ✅ **Fixed runtime issues for Vite SSR sample rendering**
   - Added `features = ["transpile"]` to `deno_runtime` — enables TypeScript transpilation for `deno_telemetry` extension sources
   - Added `features = ["hmr"]` to `deno_runtime` — makes `op_snapshot_options` use `try_take` + `unwrap_or_default` instead of panicking

8. ✅ **Vite SSR sample renders successfully**
   - `bundle exec ruby -e "require 'ssr/deno'; bundle = SSR::Deno::Bundle.new('samples/react-ssr-app/dist/server/entry-server.js'); puts bundle.render({data: {message: 'Hello World!'}})"`
   - Returns full HTML with React SSR output

9. ✅ **Added integration test**
   - [`test/ssr/test_deno_bundle.rb`](../test/ssr/test_deno_bundle.rb) — tests `Bundle.new`, `render`, `reload`, auto-reload, raw I/O modes
   - All tests pass with Rubocop compliance

10. ✅ **Dotenv-based environment configuration**
    - V8 build env vars moved from [`bin/compile`](../bin/compile) to [`.env`](../.env) (gitignored)
    - [`.env.example`](../.env.example) committed as template
    - [`dotenv`](https://rubygems.org/gems/dotenv) gem loads `.env` in [`Rakefile`](../Rakefile)
    - [`bin/compile`](../bin/compile) removed — just run `bundle exec rake compile`

11. ✅ **Compiled and verified**
    - `bundle exec rake compile` — builds with 0 warnings, 0 errors
    - `bundle exec ruby -e "require 'ssr/deno'; puts SSR::Deno.native_version"` — returns `0.1.0-alpha.1`
    - `bundle exec rake test` — all tests pass

12. ✅ **Versioned and tagged**
    - Version bumped to `0.1.0-alpha.1` in [`lib/ssr/deno/version.rb`](../lib/ssr/deno/version.rb) and [`ext/ssr_deno/Cargo.toml`](../ext/ssr_deno/Cargo.toml)
    - Gemspec populated with summary, description, and rubygems.org push host
    - README rewritten with usage instructions, development guide, and architecture reference
    - Git tag `v0.1.0-alpha.1` created

### Phase 3: Multi-Bundle & Rails Integration ✅
- Refactored from single `init_runtime`/`render` to `SSR::Deno::Bundle` class with per-bundle IDs
- Added `Bundle::Registry` for thread-safe named bundle storage
- Added `native_load_bundle(bundle_id, bundle_path)` for dynamic bundle loading
- Added `DenoError` typed error enum in Rust, mapped to Ruby exception hierarchy
- Added `ActiveSupport::Notifications` instrumentation (`render.ssr_deno`, `bundle_load.ssr_deno`, `bundle_miss.ssr_deno`)
- Added Rails integration: `Railtie`, `Helper` (`ssr_render`), `InstallGenerator`
- Added security hardening: `Permissions::none_without_prompt()`, `NoopModuleLoader`, symlink-escape check, TOCTOU fix, path redaction in errors
- Added `test/ssr/test_deno_bundle.rb`, `test/ssr/test_deno_registry.rb`, `test/ssr/test_deno_errors.rb`, `test/ssr/test_deno_concurrency.rb`, `test/ssr/test_deno_install_generator.rb`, `test/ssr/integration_deno_rails.rb`

### Phase 4: Future work
- Timeout / cancellation for runaway JS renders
- Content-Security-Policy nonce support for inline `<script>` tags in SSR output
- Document deployment considerations (V8 binary size, memory)
- Streaming SSR via `renderToPipeableStream` + `ActionController::Live`
- Template handler (`.ssr` files with YAML frontmatter)
- CI for Rust compilation (Linux is the only currently supported platform)

## Key Design Decisions

1. **`MainWorker` over `JsRuntime`**: We use `deno_runtime::MainWorker::bootstrap_from_options` instead of bare `deno_core::JsRuntime`. Frontend frameworks like React 19 depend on Web APIs (`MessageChannel`, `setTimeout`, `performance`, `console`) that are only available through Deno's extension system. `MainWorker` provides all standard Deno extensions automatically.

2. **`bootstrap_from_options` over `bootstrap`**: `MainWorker::from_options` (which does the actual construction) is private. `bootstrap_from_options` is the only public constructor that combines construction + JS bootstrap. The separate `bootstrap` method exists but requires a pre-constructed `MainWorker`.

3. **Generic type parameters**: `bootstrap_from_options` requires three generic types (`TInNpmPackageChecker`, `TNpmPackageFolderResolver`, `TExtNodeSys`). Even though we don't use npm packages, these types must be provided at compile time. We created NOP implementations that satisfy the trait bounds with minimal behavior.

4. **Isolate Pool**: An `IsolatePool` of up to 8 `IsolateHandle`s (one per V8 isolate) dispatches render requests round-robin. Each handle has its own Tokio runtime + `MainWorker` on a dedicated `deno-worker-{index}` thread. Bundles are broadcast to all isolates at load time.

5. **Web Worker Target**: Using `ssr.target: "webworker"` in Vite produces a bundle that only uses Web APIs, which Deno supports natively without Node.js compatibility layers.

6. **Self-Contained Bundle via `ssr.noExternal: true`**: This is the most critical Vite configuration option. Without it, Vite produces a bundle with external `import` statements for dependencies. The embedded Deno runtime cannot resolve these. With `noExternal: true`, Vite's rolldown inlines **all** dependencies into a single self-contained file with zero `import` statements.

7. **JSON Bridge**: Data is serialized to JSON at the Ruby boundary and deserialized in JavaScript. This keeps the interface simple and language-agnostic.

8. **Dedicated worker threads**: Each `IsolateHandle` runs a `MainWorker`
   (and its `current_thread` Tokio runtime + `LocalSet`) on a dedicated
   `"deno-worker-{index}"` OS thread. The Ruby thread only holds
   an `mpsc::Sender<WorkerMsg>` and uses `oneshot` channels for replies.
   This removes the need for `unsafe impl Send/Sync` and `UnsafeCell`, and
   makes the design robust against Ractor usage.

9. **Configuration via Ruby**: All configuration (heap limit, pool size, bundle path) is done from Ruby side, keeping the Rust extension stateless and simple. The `Mutex<Config>` pattern allows multiple configuration fields to be set independently before initialization.

10. **V8 Scope API**: The `rusty_v8` crate's scope API uses `ScopeStorage<T>` / `PinnedRef<'_, T>` / `ContextScope` pattern. `HandleScope::new(isolate)` returns `ScopeStorage<HandleScope>`, `.init()` returns `PinnedRef<HandleScope>`, and `ContextScope::new(&mut scope, context)` enters the V8 context.
