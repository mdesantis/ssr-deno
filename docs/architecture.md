# SSR-Deno Architecture

Server-side rendering for Ruby using an embedded Deno V8 runtime.

---

## Overview

```mermaid
flowchart TB
    subgraph BuildTime["Build Time"]
        Vite["Vite + React/Vue/Svelte"] --> Dist["dist/server/entry-server.js"]
    end
    Dist -->|loaded by| RubyProcess
    subgraph RubyProcess["Ruby Process"]
        RubyApp["Ruby App"] --> Bundle["Bundle.new(path).render(data)"]
        Bundle --> NativeExt["Ruby Native Extension (magnus)"]
        NativeExt -->|JSON bridge| Pool["IsolatePool<br/>(up to 8 isolates, round-robin)"]
        Pool --> Isolate["V8 Isolate<br/>deno_runtime::MainWorker<br/>globalThis.render()<br/>HTML string out"]
        Isolate --> NativeExt
        NativeExt --> Bundle
        Bundle --> RubyApp
    end
```

---

## Components

### Ruby API Layer

| File | Purpose |
|------|---------|
| `lib/ssr/deno.rb` | Module `SSR::Deno` — config setters/getters (`max_heap_size_mb`, `isolate_pool_size`, `render_timeout_ms`, `node_builtins_enabled?`), env var defaults (`SSR_DENO_*` prefix), and `heap_stats` / `heap_stats!` |
| `lib/ssr/deno/bundle.rb` | `Bundle.new(path)` → loads bundle into all isolates. `bundle.render(data)` → JSON-serializes data, dispatches to next isolate, parses result |
| `lib/ssr/deno/bundle/registry.rb` | Thread-safe `Registry` for named bundles, used by Rails integration |
| `lib/ssr/deno/instrumenter.rb` | `ActiveSupport::Notifications` wrapper (`render.ssr_deno`, `bundle_load.ssr_deno`) |
| `lib/ssr/deno/rails/railtie.rb` | Railtie — config via `config.ssr_deno`, auto-reload in dev |
| `lib/ssr/deno/rails/helper.rb` | View helper `ssr_render(data)` |

Config setters write to a Rust `Mutex<Config>` and must be called **before** the first `Bundle.new` (which triggers pool init).

### Rust Native Extension (`ext/ssr_deno/`)

| File | Purpose |
|------|---------|
| `src/lib.rs` | magnus entrypoint — registers methods on `SSR::Deno`, owns `POOL: OnceLock<IsolatePool>` and `CONFIG: Mutex<Config>` with double-checked locking |
| `src/deno_runtime_wrapper/mod.rs` | `DenoError` enum, `IsolateHandle` (channel to worker thread), `IsolatePool` (round-robin dispatcher), `build_worker`, `load_bundle_in_worker`, `setup_require` |
| `src/deno_runtime_wrapper/call_render.rs` | `call_render` — V8 scope chain, sync/async render dispatch, promise polling. `collect_heap_stats` |
| `src/sys.rs` | `Sys` type implementing `BaseFsCanonicalize`, `BaseFsMetadata`, `BaseFsRead`, `FsOpen`, `EnvCurrentDir`, etc. for `ExtNodeSys` and `WhichSys` |
| `src/nop_types.rs` | NOP implementations for `InNpmPackageChecker`, `NpmPackageFolderResolver`, `PermissionDescriptorParser` |
| `src/node_builtin_loader.rs` | Custom `ModuleLoader` that allows `node:` scheme URLs (used when `node_builtins_enabled`) |
| `src/require_loader.rs` | Minimal `NodeRequireLoader` — rejects file loading, passes built-in module resolution to Deno |
| `crates/ssr_deno_core/src/lib.rs` | Pure-Rust types: `Config`, `DenoError`, validators (`validate_pool_size`, `validate_render_timeout_ms`, `resolve_pool_size`), `next_index` counter |

### Isolate Pool

```mermaid
flowchart LR
    subgraph RubyThread["Ruby Thread"]
        Bundle["bundle.render(data)"] --> Pick["round-robin pick"]
    end
    Pick --> H1["IsolateHandle 0"]
    Pick --> H2["IsolateHandle 1"]
    Pick --> H3["IsolateHandle ... N (max 8)"]
    H1 --> W1["deno-worker-0<br/>MainWorker + V8"]
    H2 --> W2["deno-worker-1<br/>MainWorker + V8"]
    H3 --> W3["deno-worker-N<br/>MainWorker + V8"]
```

- Pool size defaults to `CPU_cores - 1` (capped at 8), reserving one core for Ruby.
- Each isolate has its own V8 heap (configured by `max_heap_size_mb`).
- Each isolate registers a `near_heap_limit_callback` that doubles the heap limit and terminates JS execution when the heap approaches the cap, turning a potential `SIGTRAP` crash into a catchable `JsRuntimeOutOfMemoryError` (see [`plans/v8-oom-protection.md`](../plans/v8-oom-protection.md)).
- Bundles are broadcast to all isolates at load time (each isolate calls `execute_script` + namespacing).
- Render requests are dispatched via atomic counter increment + channel send. No locks in the hot path.
- Render timeout is enforced via `SyncSender::recv_timeout` on the Ruby side.

### Worker Thread Lifecycle

1. `IsolateHandle::spawn` creates an OS thread with a `current_thread` Tokio runtime + `LocalSet`.
2. `build_worker` constructs a `MainWorker` via `bootstrap_from_options` with:
   - `Permissions::none_without_prompt()` — all Deno permissions denied.
   - `NoopModuleLoader` (or `NodeBuiltinOnlyModuleLoader` if `node_builtins_enabled`).
   - `NodeExtInitServices` (if `node_builtins_enabled`) — provides `NodeRequireLoader`, `NodeResolver`, `PackageJsonResolver` for the `deno_node` extension.
   - A `near_heap_limit_callback` registered on the V8 isolate — doubles the heap limit and terminates execution when the heap approaches `max_heap_size_mb`, preventing fatal process crash on user memory leaks.
3. The worker thread runs a message loop processing `LoadBundle`, `Render`, and `HeapStats` messages.
4. Bundles are evaluated via `MainWorker::execute_script` (synchronous V8 script execution, not module loading).

### Bundle Contract

A Vite SSR bundle must expose a `globalThis.render(argsJson: string): string` function:

```ts
function render(argsJson: string): string {
  const data = JSON.parse(argsJson)
  // ... render to HTML ...
  return html
}
globalThis.render = render
```

- `argsJson` is a JSON string passed from Ruby (auto-serialized by `bundle.render`).
- The return value must be an HTML string (or a Promise resolving to one).
- The Rust runtime auto-detects async (`v8::Promise`) returns and polls the microtask queue.

### SSR task type limitations

The SSR pipeline runs `execute_script` + `perform_microtask_checkpoint` but
NEVER runs the V8 event loop. This means only **microtasks** are dispatched;
**macrotasks** are silently queued and never executed.

| Category | APIs that work | APIs that silently never fire |
|---|---|---|
| **Microtasks** | `Promise.then`, `queueMicrotask`, `async/await` | — |
| **Macrotasks** | — | `setTimeout`, `setInterval`, `MessagePort`, `fetch`, `requestAnimationFrame` |

React 19's streaming SSR (`renderToPipeableStream`, `renderToReadableStream`)
requires `MessagePort` (a macrotask) and cannot work without the event loop.
Vue 3 async SSR works because it uses only Promises (microtasks). See
[`plans/macrotasks-in-ssr.md`](../plans/macrotasks-in-ssr.md) for details.

**Recommended bundler settings (Vite example):**
- `ssr.noExternal: true` — bundles all dependencies into a single self-contained file.
- `ssr.target: 'webworker'` — produces a bundle using only Web APIs (safe default; not a gem requirement).
- `ssr.resolve.conditions: ['edge-light', 'module', 'browser', 'development']` — prevents packages like `@emotion/cache` from resolving to their browser-specific build.

See `samples/` for 12 complete working examples: barebone (plain JS), deno-native (no Vite), vanilla TS, React 19, React 19 streaming, Vue 3, Svelte 5, Preact, MUI v9, Emotion CSS, and a full dashboard.

---

## Node.js Builtin Support

**Disabled by default.** Enable with `SSR::Deno.node_builtins_enabled = true` before pool init.

When enabled:
1. `build_worker` uses `NodeBuiltinOnlyModuleLoader` (allows `node:` scheme URLs) instead of `NoopModuleLoader`.
2. `build_worker` initializes `NodeExtInitServices` with a `NodeRequireLoader`, `NodeResolver`, and `PackageJsonResolver`.
3. Before each bundle evaluation, `setup_require` runs an async `import('node:module')` and polls the microtask queue with a 10ms deadline until `globalThis.require` is available via `createRequire`; raises `BundleLoad` error if the import fails.

This allows CJS bundles that call `require("stream")`, `require("buffer")`, `require("events")`, etc. to work in the embedded V8 context. Packages like `@emotion/server` that depend on Node.js built-in modules via `through2` → `multipipe` → `html-tokenize` can be used without manual CSS extraction.

**Cost:** ~50ms added to worker initialization (one-time per isolate).

---

## Testing

Tests run in separate Ruby processes to avoid pool re-initialization
between suites. Each suite sets its own config before pool init:

| Suite | Config differences | Covers |
|-------|-------------------|--------|
| `test:main` | Defaults | Core, all integrations, stability |
| `test:setters` | `max_heap_size_mb=128`, `pool_size=2` | Setter guards before/after init |
| `test:node_builtins` | `node_builtins_enabled=true`, `render_timeout_ms=2000` | Node builtin modules |
| `test:async` | `render_timeout_ms=100` | Async render, promise polling |
| `test:env_config` | Env vars only | `SSR_DENO_*` env var loading |

All suites run via `bundle exec rake test` (or as part of `bundle exec rake`).
Each sets `SimpleCov.command_name` for a distinct key in `.resultset.json`.
The final merge validates combined coverage at **100% line + 100% branch**.

---

## Configuration Flow

```mermaid
sequenceDiagram
    participant Ruby as Ruby App
    participant Config as CONFIG (Mutex)
    participant Pool as IsolatePool
    participant Worker as deno-worker-N

    Ruby->>Config: max_heap_size_mb = 128
    Ruby->>Config: isolate_pool_size = 4
    Ruby->>Config: node_builtins_enabled = true
    Ruby->>Pool: Bundle.new(path)
    Note over Pool: reads CONFIG snapshot
    Pool->>Pool: resolve_pool_size(4)
    loop for each isolate
        Pool->>Worker: spawn(max_heap, node_builtins)
        Worker->>Worker: build_worker(node_builtins)
        Worker-->>Pool: ready
    end
    Pool-->>Ruby: pool initialized
    Ruby->>Pool: load_bundle(path)
    Pool->>Worker: broadcast bundle code
    Worker->>Worker: execute_script(bundle)
    Worker-->>Pool: render registered
    Pool-->>Ruby: ready
```

---

## Source Files (Quick Reference)

```
ext/ssr_deno/                                         # Rust native extension
├── Cargo.toml                                        # deno_runtime, magnus dependencies
├── crates/ssr_deno_core/                             # Pure-Rust types (no V8 dep)
│   └── src/lib.rs                                    # Config, DenoError, validators
└── src/
    ├── lib.rs                                        # magnus init, CONFIG, POOL
    ├── deno_runtime_wrapper/
    │   ├── mod.rs                                    # IsolatePool, IsolateHandle, build_worker
    │   └── call_render.rs                            # call_render, heap_stats
    ├── sys.rs                                        # Sys type for Deno traits
    ├── nop_types.rs                                  # NOP implementations
    ├── node_builtin_loader.rs                        # ModuleLoader for node: scheme
    └── require_loader.rs                             # NodeRequireLoader for builtins

lib/ssr/deno/                                         # Ruby module
├── deno.rb                                           # Core entry point, config setters
├── version.rb                                        # VERSION
├── bundle.rb                                         # Bundle class
├── bundle/registry.rb                                # Thread-safe bundle storage
├── instrumenter.rb                                   # Notifications wrapper
├── rails.rb                                          # Rails integration entry point
└── rails/                                            # Railtie, helper, generator

sig/ssr/deno.rbs                                      # RBS type signatures

test/
├── test_helper.rb                                    # SimpleCov, pool config
├── ssr/test_deno*.rb                                 # Unit tests (Bundle, errors, etc.)
├── ssr/test_integration_samples.rb                   # Integration tests (all samples)
└── ssr/test_integration_node_builtins.rb             # node_builtins integration test

rakelib/
├── cargo.rake                                        # cargo:test
├── samples.rake                                      # samples:build
└── test.rake                                         # test:main, test:node_builtins

samples/
├── barebone-ssr-app/                                 # Plain JS, zero deps
├── deno-native-ssr-app/                              # Deno.serve() + template strings, no build
├── deno-native-react-ssr-app/                        # Deno.serve() + React 19, no build
├── vite-ssr-app/                                     # Plain TS + Vite
├── vite-react-ssr-app/                               # React 19 + Vite
├── vite-react-streaming-ssr-app/                     # React 19 streaming SSR + Vite
├── vite-react-mui-ssr-app/                           # React 19 + MUI v9 + Vite
├── vite-react-mui-emotion-ssr-app/                   # React 19 + MUI v9 + Emotion CSS + Vite
├── vite-react-emotion-mui-dashboard-ssr-app/         # Full dashboard + Vite
├── vite-vue-ssr-app/                                 # Vue 3 + Vite
├── vite-svelte-ssr-app/                              # Svelte 5 + Vite
├── vite-preact-ssr-app/                              # Preact + Vite
├── webpack-ssr-app/                                  # Plain TS + Webpack 5
├── webpack-react-ssr-app/                            # React 19 + Webpack 5
└── node-ssr-app/                                     # Plain TS + esbuild (Node.js)
```
