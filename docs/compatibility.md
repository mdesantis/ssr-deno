# SSR Compatibility

What the `ssr-deno` gem supports and does not support for server-side rendering
in an embedded V8 isolate.

---

## Framework Support

| Framework | SSR method | Status | Notes |
|---|---|---|---|
| React 19 | `renderToString` | ✅ | Synchronous, fully supported |
| React 19 | `renderToPipeableStream` | ⚠️ | Event loop runs with `render(event_loop: true)`, but JS-side streaming plumbing (Writable, pipe, chunk collection) must be added to the bundle |
| React 19 | `renderToReadableStream` | ⚠️ | Same as `renderToPipeableStream` |
| Vue 3 | `renderToString` | ✅ | Async (Promise-based), works via microtask polling |
| Preact | `renderToString` | ✅ | Synchronous, fully supported |
| Svelte 5 | `renderToString` | ✅ | Synchronous, fully supported |
| SolidJS | `renderToString` | ✅ | Synchronous (returns string) |
| Plain JS/TS | `globalThis.render()` | ✅ | Any function returning a string or Promise |
| Any | `Bundle#render(event_loop: true)` | ✅ | Runs the V8 event loop during render. Supports `setTimeout`, `MessagePort`, and macrotask-based APIs. Alias: `Bundle#render_stream`. |

If your framework is not listed, it works if it:
- Exposes a synchronous JS function that returns an HTML string, or
- Returns a `Promise` that resolves to an HTML string

**Macrotasks:** `setTimeout` and `MessagePort` work with
`Bundle#render(event_loop: true)` (or its alias `render_stream`) but NOT by
default (`event_loop: false`). See the
[Macrotask-based APIs](#macrotask-based-apis-without-eventloop-true)
section for details.

It does NOT work if it depends on:
- `fetch` — network permissions denied regardless of event loop
- The event loop running continuously in the background
- ES module dynamic imports during render

---

## JS API Compatibility

### Standard built-ins (always available)

| API | Supported | Notes |
|---|---|---|
| `Promise` / `async`/`await` | ✅ | Full support — microtask queue is polled |
| `queueMicrotask` | ✅ | Microtask, dispatched during polling |
| `JSON.parse` / `JSON.stringify` | ✅ | Used internally for data serialization |
| `Math` / `Date` / `String` / `Array` / `Object` | ✅ | Standard V8 builtins |
| `Map` / `Set` / `WeakMap` / `WeakSet` | ✅ | |
| `TypedArray` / `ArrayBuffer` / `DataView` | ✅ | |
| `Error` / `TypeError` / `RangeError` / `SyntaxError` | ✅ | |
| `Symbol` / `BigInt` | ✅ | |
| `RegExp` | ✅ | |
| `Proxy` / `Reflect` | ✅ | |
| `Intl` / `Intl.NumberFormat` / `Intl.DateTimeFormat` | ✅ | V8's ICU data is available |

### Web APIs (provided by Deno runtime)

| API | Supported | Notes |
|---|---|---|
| `URL` / `URLSearchParams` | ✅ | Included in Deno Web API extensions |
| `TextEncoder` / `TextDecoder` | ✅ | |
| `console.log` / `console.error` / `console.warn` | ✅ | Outputs to process stderr |
| `globalThis` | ✅ | The JS context global object |
| `AbortController` / `AbortSignal` | ✅ | |
| `EventTarget` / `Event` | ✅ | |
| `Performance` / `performance.now()` | ✅ | |
| `structuredClone` | ✅ | |
| `atob` / `btoa` | ✅ | Base64 encoding |

### Macrotask-based APIs (without `event_loop: true`)

`Bundle#render` with default `event_loop: false` uses `execute_script` + `perform_microtask_checkpoint`
and never runs the V8 event loop. Macrotask callbacks are silently queued and
never executed in the default path.

**Use `Bundle#render(event_loop: true)` (or its alias `Bundle#render_stream`) to
enable macrotask dispatch.** This runs the V8 event loop during rendering,
allowing `setTimeout`, `setInterval`, and `MessagePort` callbacks to fire.
`setImmediate` is a special case — it's wired to a libuv check watcher that is
not available in our tokio-based embedding, so its callbacks never fire even
with the event loop. Use `setTimeout(fn, 0)` as a replacement.

See [`plans/macrotasks-in-ssr.md`](../plans/macrotasks-in-ssr.md) for the
architectural details.

| API | Supported | Notes |
|---|---|---|
| `setTimeout` / `clearTimeout` | ⚠️ | Macrotask — works with `render(event_loop: true)`, not with `render` (default) |
| `setInterval` / `clearInterval` | ⚠️ | Macrotask — same limitation as `setTimeout` |
| `fetch` | ❌ | I/O op — network permissions denied regardless |
| `MessagePort` / `postMessage` | ⚠️ | Macrotask — works with `render(event_loop: true)`. React 19 streaming uses this internally but also needs JS-side streaming setup. |
| `requestAnimationFrame` | ❌ | Macrotask — browser-only anyway |
| `setImmediate` / `clearImmediate` | ❌ | Macrotask — wired to libuv check watcher, not available in our tokio-based embedding. Even the event loop can't dispatch these. Use `setTimeout(fn, 0)` for a similar pattern. |
| `process.nextTick` | ❌ | Not available in Web API context |
| `WebSocket` | ❌ | Requires event loop |
| `createServer` / `http` / `https` | ❌ | Network I/O |

### Deno-specific APIs (NOT available)

The runtime is initialized with `Permissions::none_without_prompt()`, denying all
Deno permissions. Deno-specific APIs are not registered.

| API | Supported | Notes |
|---|---|---|
| `Deno.readFile` / `Deno.writeFile` | ❌ | All permissions denied |
| `Deno.serve` / `Deno.listen` | ❌ | Network permissions denied |
| `Deno.env` | ❌ | Environment access denied |
| `Deno.exit` | ❌ | Process control denied |
| `Deno.cwd` / `Deno.chdir` | ❌ | Filesystem access denied |
| `Deno.build` / `Deno.version` | ❌ | Not registered |
| `Deno.consoleSize` | ❌ | TTY access denied |

### Node.js builtins (conditional)

Enabled via `SSR::Deno.node_builtins_enabled = true` or
`SSR_DENO_NODE_BUILTINS_ENABLED=true` before pool init.

| API | Supported | Notes |
|---|---|---|
| `require("stream")` | ✅ | Only with `node_builtins_enabled` |
| `require("buffer")` | ✅ | Only with `node_builtins_enabled` |
| `require("events")` | ✅ | Only with `node_builtins_enabled` |
| `require("string_decoder")` | ✅ | Only with `node_builtins_enabled` |
| `require("path")` | ✅ | Only with `node_builtins_enabled` |
| `require("url")` | ✅ | Only with `node_builtins_enabled` |
| `require("fs")` | ❌ | File system access denied regardless |
| `require("net")` / `require("http")` | ❌ | Network access denied regardless |
| `require("child_process")` | ❌ | Process spawning denied regardless |
| `require("./relative-file.js")` | ❌ | File loading via `require()` is explicitly rejected |
| `require("/absolute/path.js")` | ❌ | File loading via `require()` is explicitly rejected |

---

## Known Limitations

### Module loading

Bundles are loaded via `execute_script` (synchronous V8 script execution), NOT
via ES module resolution. This means:

- **`import` statements** at the top level must be compiled away by the bundler
  into a single self-contained file.
- **Dynamic `import()`** during render is rejected — the module loader is not
  available at runtime.
- **`require()` for file paths** (`require("./relative.js")`,
  `require("/absolute.js")`) is explicitly rejected by the custom
  `DenoNodeRequireLoader`. All npm dependencies must be inlined at build time.
- **`require()` for Node.js builtins** (`require("stream")`) works only when
  `node_builtins_enabled = true`. The `NodeBuiltinOnlyModuleLoader` resolves
  `node:` scheme specifiers; everything else returns an error.

The two module loaders:

| Loader | Used when | Allows |
|---|---|---|
| `NoopModuleLoader` | `node_builtins: false` (default) | Nothing — bundles must not use `import`/`require` at runtime |
| `NodeBuiltinOnlyModuleLoader` | `node_builtins: true` | `node:` scheme specifiers only (stream, buffer, events, etc.) |

### Macrotask starvation (without `event_loop: true`)

`setTimeout`, `setInterval`, `MessagePort`, and `fetch` callbacks never fire
in `Bundle#render` with default `event_loop: false` (which uses `execute_script` +
`perform_microtask_checkpoint` only). Only microtasks (`Promise.then`,
`queueMicrotask`, `async/await`) are dispatched.

**Partial fix:** `Bundle#render(event_loop: true)` runs the V8 event loop during
rendering, which dispatches macrotasks like `setTimeout`, `setInterval`, and
`MessagePort`. Use `render(event_loop: true)` (or its alias `render_stream`)
when your SSR code depends on timers or async scheduling. `fetch` is still not
supported (network permissions denied regardless of event loop).

React 19 streaming SSR (`renderToPipeableStream`, `renderToReadableStream`)
requires the event loop and uses `MessagePort` internally. With
`render(event_loop: true)`, the event loop runs, but the JS-side streaming
plumbing (Writable, pipe, chunk collection) must be set up in the bundle. See
[`plans/event-loop-approach-c.md`](../plans/event-loop-approach-c.md).

### Bundle code footprint

Bundle code is loaded via `execute_script` and stays in V8 heap for the process
lifetime. Only calling `Bundle#reload` replaces it. There is no "unload" API.

Each SSR bundle includes its own copy of React. With multiple bundles,
React is compiled independently in each one. With a pool of N isolates,
the total bundle memory cost is `bundles × isolates × ~3 MB`.

### Heap limits

`max_heap_size_mb` is a **per-isolate** constraint, not a total process budget.
With `pool_size = 4` and `max_heap_size_mb = 64`, V8 may allocate up to
`4 × 64 = 256 MB` combined. The auto-detect default (`CPU - 1`, max 8) can
be aggressive on high-core machines.

A user component that leaks memory across renders triggers the
near-heap-limit callback, which terminates execution and raises
`SSR::Deno::JsRuntimeOutOfMemoryError`. This prevents the process crash
that would otherwise occur when V8 hits the heap limit.

### OOM behavior

Before pool init, V8's default heap limit (1.4 GB on 64-bit) applies. After
pool init, each isolate uses the configured `max_heap_size_mb`. If a render
approaches the limit:

1. V8 GC runs a last-resort mark-compact
2. The near-heap-limit callback fires, doubles the limit, and terminates JS
3. `call_render` maps the termination to `DenoError::OutOfMemory`
4. Ruby receives `SSR::Deno::JsRuntimeOutOfMemoryError`

The process does NOT crash with `SIGTRAP` (unlike a bare V8 embedding without
the callback). See [`plans/archived/v8-oom-protection.md`](../plans/archived/v8-oom-protection.md).

### Worker death

Once the isolate pool is initialized (first `Bundle.new`), it is permanent.
There is no public API to tear down and reinitialize the pool. If an isolate
worker thread exits unexpectedly, `native_render` returns
`SSR::Deno::JsRuntimeWorkerError`. The pool itself remains alive; renders
are dispatched to the remaining isolates via round-robin.
