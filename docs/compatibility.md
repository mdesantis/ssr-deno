# SSR Compatibility

What the `ssr-deno` gem supports and does not support for server-side rendering
in an embedded V8 isolate.

---

## Framework Support

| Framework | SSR method | Status | Notes |
|---|---|---|---|
| React 19 | `renderToString` | ✅ | Synchronous, fully supported |
| React 19 | `renderToPipeableStream` | ✅ | Full event loop support — JS-side streaming plumbing (Writable, pipe, chunk collection) must be added to the bundle |
| React 19 | `renderToReadableStream` | ✅ | Same as `renderToPipeableStream` |
| Vue 3 | `renderToString` | ✅ | Async (Promise-based), works via microtask polling |
| Preact | `renderToString` | ✅ | Synchronous, fully supported |
| Svelte 5 | `renderToString` | ✅ | Synchronous, fully supported |
| SolidJS | `renderToString` | ✅ | Synchronous (returns string) |
| Plain JS/TS | `globalThis.render()` | ✅ | Any function returning a string or Promise |
| Any | `Bundle#render` | ✅ | Always runs the V8 event loop. Supports `setTimeout`, `MessagePort`, and macrotask-based APIs. |

If your framework is not listed, it works if it:
- Exposes a synchronous JS function that returns an HTML string, or
- Returns a `Promise` that resolves to an HTML string

**Macrotasks:** `setTimeout` and `MessagePort` always work — the event loop
runs during every render.

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

### Macrotask-based APIs

The V8 event loop runs during every render, so macrotasks dispatch normally.
`setImmediate` is a special case — it's wired to a libuv check watcher that is
not available in our tokio-based embedding, so its callbacks never fire.
Use `setTimeout(fn, 0)` as a replacement.

See [`plans/macrotasks-in-ssr.md`](../plans/macrotasks-in-ssr.md) for the
architectural details.

| API | Supported | Notes |
|---|---|---|
| `setTimeout` / `clearTimeout` | ✅ | Macrotask — fires during render |
| `setInterval` / `clearInterval` | ✅ | Macrotask — fires during render |
| `fetch` | ❌ | I/O op — network permissions denied regardless |
| `MessagePort` / `postMessage` | ✅ | Macrotask — fires during render. React 19 streaming uses this internally. |
| `requestAnimationFrame` | ❌ | Macrotask — browser-only anyway |
| `setImmediate` / `clearImmediate` | ❌ | Macrotask — wired to libuv check watcher, not available in our tokio-based embedding. Use `setTimeout(fn, 0)` for a similar pattern. |
| `process.nextTick` | ❌ | Not available in Web API context |
| `WebSocket` | ❌ | Requires network permissions |
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

### Synchronous blocking JS and timeouts

The render timeout is enforced by a dedicated watchdog thread (`Watchdog` in
`render.rs`) that calls `v8::IsolateHandle::terminate_execution()` after the
configured deadline. This interrupts both synchronous blocking JS (e.g.,
`while(Date.now() < end) {}`) and hung async renders (Promises that never
resolve). After termination, `cancel_terminate_execution()` restores the isolate
for reuse on subsequent render requests.

React 19 streaming SSR (`renderToPipeableStream`, `renderToReadableStream`)
works out of the box — the event loop runs during every render and
`MessagePort` dispatches naturally.

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
3. The render function detects the OOM flag and maps it to `SSRDenoError::OutOfMemory`
4. Ruby receives `SSR::Deno::JsRuntimeOutOfMemoryError`

The process does NOT crash with `SIGTRAP` (unlike a bare V8 embedding without
the callback). See [`plans/archived/v8-oom-protection.md`](../plans/archived/v8-oom-protection.md).

### Worker death

Once the isolate pool is initialized (first `Bundle.new`), it is permanent.
There is no public API to tear down and reinitialize the pool. If an isolate
worker thread exits unexpectedly, `native_render` returns
`SSR::Deno::JsRuntimeWorkerError`. The pool itself remains alive; renders
are dispatched to the remaining isolates via round-robin.
