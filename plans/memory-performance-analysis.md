# SSR Memory & Performance Analysis

> Analysis of the [`ssr-deno`](../lib/ssr/deno.rb:1) gem's SSR architecture when integrated into a Rails application via [`lib/ssr/deno/rails.rb`](../lib/ssr/deno/rails.rb:1).

---

## 1. Architecture Overview

The SSR pipeline has three layers:

```
Ruby (Rails) ──blocking_send──> Rust (tokio channel) ──> Deno Worker Thread (V8 isolate)
```

- **Up to N V8 isolates** (default: CPU count, max 8) per Ruby process via `IsolatePool`, initialized lazily on first [`Bundle.new`](../lib/ssr/deno/bundle.rb:16)
- **One background thread** (`deno-worker`) running a tokio runtime with a `LocalSet`
- **Multiple bundles** coexist in the same V8 context under `globalThis.__ssr_bundles[bundle_id]`
- **Serialized communication** via `tokio::sync::mpsc::channel(1)` — buffer depth of 1
- **Ractor-safe** — the Rust extension declares `rb_ext_ractor_safe(true)`, and the tokio channel serializes concurrent requests

---

## 2. Memory Analysis

### 2.1 V8 Isolate Baseline

The single V8 isolate is the dominant memory cost. Breakdown:

| Component | Estimated Size | Notes |
|---|---|---|
| V8 heap (empty isolate) | ~4–8 MB | Default young + old space |
| Deno runtime Web APIs | ~8–12 MB | `fetch`, `setTimeout`, `URL`, `TextEncoder`, etc. |
| Tokio runtime + thread stack | ~2–4 MB | Default 2 MB thread stack + heap |
| Rust FFI glue + magnus | ~1–2 MB | Ruby↔Rust bridge |
| **Subtotal (idle)** | **~15–26 MB** | Per Ruby process |

### 2.2 Bundle Code in V8 Heap

Each loaded bundle adds JavaScript code to the V8 heap:

| Component | Estimated Size | Notes |
|---|---|---|
| React 19 UMD (minified) | ~130 KB | `react` + `react-dom/server` bundled by Vite |
| Application code | ~10–100 KB | Components, routes, stores |
| Vite polyfills/wrappers | ~20–50 KB | Module wrapping, import shims |
| **Per bundle (parsed+compiled)** | **~2–5 MB** | V8's compiled bytecode + internalized strings |
| **Per bundle (source text)** | **~160–280 KB** | Raw JS source retained for stack traces |

**Key insight:** V8 compiles JS to internal bytecode which is ~10–20x larger than source. A 200 KB bundle becomes ~2–4 MB in V8 heap after parsing.

### 2.3 Multiple Bundles

Multiple bundles share the same V8 isolate. React is bundled independently per Vite build, so:

- **2 bundles** (e.g., `:application` + `:admin`): ~4–10 MB additional (React duplicated in each bundle's compiled code)
- **No deduplication** — Vite bundles are self-contained; React's `renderToString` is compiled twice

### 2.4 Render-Time Memory

During `renderToString`:

| Component | Estimated Size | Notes |
|---|---|---|
| VDOM tree (intermediate) | ~0.5–5 MB | Proportional to component tree depth |
| Output HTML string | ~10–100 KB | Final rendered HTML |
| JSON serialization buffer | ~1–10 KB | `JSON.stringify` of result |
| **Per render (peak)** | **~0.5–5 MB** | Freed by V8 GC after call completes |

### 2.5 Total Memory Budget

| Scenario | Estimated RSS | Notes |
|---|---|---|
| Rails app (baseline, no SSR) | ~100–200 MB | Typical Puma worker |
| Rails + ssr-deno (idle, 1 bundle) | ~115–226 MB | +15–26 MB V8 isolate |
| Rails + ssr-deno (idle, 2 bundles) | ~119–236 MB | +4–10 MB for second bundle |
| Rails + ssr-deno (peak render) | ~120–241 MB | +0.5–5 MB transient VDOM |
| Rails + ssr-deno (multi-threaded) | ~120–241 MB | Same isolate, serialized access |

### 2.6 Memory Concerns

1. **No per-request isolation** — All renders share the same V8 context. A memory-leaking component (e.g., accumulating event listeners, growing caches) affects all subsequent renders until GC runs.

2. **Bundle code is never unloaded** — Once loaded via [`load_bundle_in_worker`](../ext/ssr_deno/src/deno_runtime_wrapper.rs:230), the code stays in V8 heap for the process lifetime. Only `reload` replaces it.

3. **`Box::leak` for script names** — At [`deno_runtime_wrapper.rs:131`](../ext/ssr_deno/src/deno_runtime_wrapper.rs:131), each bundle load leaks the script filename string. At ~50 bytes per leak × bundle reloads in development, this is negligible (~5 KB after 100 reloads).

4. **V8 GC pressure** — V8's GC runs independently of Ruby's GC. Under high request throughput, V8 may accumulate garbage between renders, causing periodic latency spikes.

---

## 3. Performance Analysis

### 3.1 Request Lifecycle

```
Request arrives
  │
  ▼
Rails controller action
  │
  ▼
View template calls ssr_render(data)
  │
  ▼
Bundle#render:
  ├─ [if auto_reload] stat() syscall (~1 µs)
  ├─ JSON.generate(data) (~1–10 µs for typical data)
  ├─ instrument() block
  │   └─ SSR::Deno.native_render(bundle_id, json_input)
  │       ├─ blocking_send to tokio channel (~1 µs)
  │       ├─ Worker thread deserializes message
  │       ├─ V8: lookup bundle + render function
  │       ├─ V8: call render(args_json)
  │       │   ├─ JSON.parse(args_json) in JS
  │       │   ├─ renderToString(React element)
  │       │   └─ return HTML string
  │       ├─ V8: JSON.stringify(result)
  │       ├─ Worker thread sends reply via oneshot channel
  │       └─ blocking_recv receives result
  └─ JSON.parse(result) (~1–10 µs)
  │
  ▼
HTML returned to view, rendered in layout
```

### 3.2 Latency Breakdown

| Phase | Duration | Notes |
|---|---|---|
| **Ruby serialization** (JSON.generate) | ~1–10 µs | Negligible |
| **Channel send** (blocking_send) | ~1 µs | Lock-free, fast path |
| **V8 function lookup** | ~1–5 µs | Property access on `__ssr_bundles` |
| **JS JSON.parse** | ~1–5 µs | Input data deserialization |
| **React renderToString** | **~5–50 ms** | **Dominant cost** — proportional to component tree |
| **V8 JSON.stringify** | ~1–5 µs | Output serialization |
| **Channel receive** (blocking_recv) | ~1 µs | Oneshot channel, fast path |
| **Ruby JSON.parse** | ~1–10 µs | Result deserialization |
| **Total (typical)** | **~5–50 ms** | Per request |

### 3.3 Throughput Analysis

The architecture has a **critical bottleneck**: a single V8 isolate with a channel buffer of 1.

```
Request A ──blocking_send──> [channel] ──> V8 isolate ──> reply A
Request B ──blocking_send──> [channel] ──> V8 isolate ──> reply B
                              ↑
                         Queue here
```

| Concurrency Model | Max Throughput | Notes |
|---|---|---|
| Single-threaded (1 Puma thread) | ~20–200 req/s | Limited by `renderToString` latency |
| Multi-threaded (2+ Puma threads) | ~20–200 req/s | **Same limit** — all threads serialize on the single V8 isolate |
| Multi-process (2+ Puma workers) | ~40–400 req/s | Each worker has its own V8 isolate |

**Key insight:** Adding Puma threads does NOT increase SSR throughput. The single V8 isolate is the bottleneck. To scale SSR throughput, you must scale Puma **workers** (processes), not threads.

### 3.4 React renderToString Performance

`renderToString` is synchronous and CPU-bound. Typical benchmarks:

| Component Tree | renderToString Time |
|---|---|
| Simple page (10 components) | ~5–15 ms |
| Medium page (50 components) | ~15–40 ms |
| Complex page (200+ components) | ~40–100 ms |
| Heavy page (500+ components, deeply nested) | ~100–300 ms |

### 3.5 Thread Contention

When multiple Puma threads hit SSR simultaneously:

1. **Thread A** calls `native_render` → `blocking_send` → worker picks it up
2. **Thread A** blocks on `blocking_recv` (releases GVL? **No** — magnus holds the GVL during FFI calls)
3. **Thread B** calls `native_render` → `blocking_send` → channel is full (buffer=1) → **Thread B blocks** on `blocking_send`
4. Worker finishes Thread A's render → sends reply → Thread A unblocks
5. Channel has space → Thread B's `blocking_send` completes → worker picks it up

**Result:** Threads are serialized at the channel level. No parallelism, but also no data corruption.

### 3.6 Ractor Performance

Ractors provide true parallelism (separate GVL), but they still serialize on the single V8 isolate:

```
Ractor 1 ──> native_render ──blocking_send──> [channel] ──> V8
Ractor 2 ──> native_render ──blocking_send──> [channel] (full) ──> blocked
```

Same bottleneck. Ractors don't help SSR throughput unless we have multiple V8 isolates.

---

## 4. Rough Calculations

### 4.1 Scenario: Typical Rails E-Commerce App

**Assumptions:**
- 4 Puma workers (processes)
- 3 threads per worker
- 1 SSR bundle (`:application`)
- Medium page (~50 components)
- `renderToString` takes ~25 ms
- 50% of requests use SSR, 50% are API/static

| Metric | Per Worker | Total (4 workers) |
|---|---|---|
| V8 isolate memory | ~20 MB | ~80 MB |
| Bundle code in V8 heap | ~3 MB | ~12 MB |
| **SSR memory overhead** | **~23 MB** | **~92 MB** |
| Rails baseline RSS | ~150 MB | ~600 MB |
| **Total RSS with SSR** | **~173 MB** | **~692 MB** |
| SSR throughput (single worker) | ~40 req/s | ~160 req/s |
| SSR P95 latency | ~35 ms | ~35 ms |

### 4.2 Scenario: High-Traffic SaaS App

**Assumptions:**
- 8 Puma workers
- 5 threads per worker
- 2 SSR bundles (`:application`, `:admin`)
- Simple page (~15 components)
- `renderToString` takes ~10 ms
- 70% of requests use SSR

| Metric | Per Worker | Total (8 workers) |
|---|---|---|
| V8 isolate memory | ~20 MB | ~160 MB |
| Bundle code (2 bundles) | ~6 MB | ~48 MB |
| **SSR memory overhead** | **~26 MB** | **~208 MB** |
| Rails baseline RSS | ~200 MB | ~1.6 GB |
| **Total RSS with SSR** | **~226 MB** | **~1.8 GB** |
| SSR throughput (single worker) | ~100 req/s | ~800 req/s |
| SSR P95 latency | ~15 ms | ~15 ms |

### 4.3 Scenario: Content Site (Blog/Docs)

**Assumptions:**
- 2 Puma workers
- 2 threads per worker
- 1 SSR bundle
- Complex page (~100 components, MDX content)
- `renderToString` takes ~40 ms
- 90% of requests use SSR

| Metric | Per Worker | Total (2 workers) |
|---|---|---|
| V8 isolate memory | ~20 MB | ~40 MB |
| Bundle code | ~4 MB | ~8 MB |
| **SSR memory overhead** | **~24 MB** | **~48 MB** |
| Rails baseline RSS | ~120 MB | ~240 MB |
| **Total RSS with SSR** | **~144 MB** | **~288 MB** |
| SSR throughput (single worker) | ~25 req/s | ~50 req/s |
| SSR P95 latency | ~55 ms | ~55 ms |

### 4.4 Cost Comparison: SSR vs CSR

| Factor | SSR (ssr-deno) | CSR (no SSR) |
|---|---|---|
| Server memory (4 workers) | +92 MB | 0 |
| Server CPU per request | +25 ms V8 work | 0 |
| Client TTFB | ~35 ms (server rendered) | ~200 ms (CDN static) |
| Client FCP | ~50 ms (pre-rendered HTML) | ~500 ms (JS must load + render) |
| Client TTI | ~500 ms (hydration) | ~800 ms (full client render) |
| SEO | ✅ Full HTML | ⚠️ Requires crawler JS support |
| Social preview | ✅ Full HTML | ⚠️ Requires pre-rendering service |

---

## 5. Bottlenecks & Risks

### 5.1 Single V8 Isolate (Primary Bottleneck)

```
┌────────────────┐     ┌──────────────┐     ┌──────────────┐
│ Puma Thread 1  │────▶              │     │              │
├────────────────┤     │  tokio::mpsc  │────▶│  V8 Isolate  │
│ Puma Thread 2  │────▶  channel(1)  │     │  (single)    │
├────────────────┤     │              │     │              │
│ Puma Thread 3  │────▶              │     └──────────────┘
└────────────────┘     └──────────────┘
```

**Impact:** SSR throughput is capped at `1 / renderToString_time` per Puma worker. For a 25 ms render, max ~40 req/s per worker regardless of thread count.

**Mitigation options:**
- Scale Puma workers (processes) — each gets its own V8 isolate
- Use a dedicated SSR process pool (separate from Puma)
- Consider multiple V8 isolates per process (future work)

### 5.2 No Request Timeout

If a `renderToString` hangs (e.g., infinite loop in component), the V8 isolate is blocked indefinitely. All subsequent SSR requests queue up and eventually time out at the Rack/HTTP layer.

**Mitigation:** Added configurable timeout on the reply channel receiver side in [`block_on_render`](../ext/ssr_deno/src/deno_runtime_wrapper.rs:154) using `std::sync::mpsc::Receiver::recv_timeout`. Configured via `SSR::Deno.render_timeout_ms=` (default 500ms).

### 5.3 V8 GC Pause

V8's garbage collector runs on the same thread as renders. A full GC (mark-sweep) can pause for 10–100 ms, blocking all SSR requests during that window.

**Mitigation:** Monitor V8 heap statistics. If GC pauses become problematic, consider:
- Isolate-per-render (expensive, high memory)
- Explicit GC triggering after large renders
- Heap size limits via V8 `CreateParams::max_old_generation_size_in_bytes` — implemented via `SSR::Deno.max_heap_size_mb=` (default 64 MB)

### 5.4 Bundle Size Bloat

Each Vite SSR bundle includes its own copy of React. With multiple bundles, React is duplicated in V8 heap:

```
Bundle A: [React 19 + App A code]  →  ~3 MB in V8 heap
Bundle B: [React 19 + App B code]  →  ~3 MB in V8 heap
                                    ─────────
                                    ~6 MB total (React counted twice)
```

**Mitigation:** If multiple bundles share the same framework, consider a shared runtime bundle that loads first, then app-specific bundles. This requires Vite federation or manual code splitting.

---

## 6. Recommendations

### 6.1 Immediate (Low Effort)

1. **Add a render timeout** (default 500ms, configurable) — implemented via `SSR::Deno.render_timeout_ms=`.

2. **Document the threading model** in README — see the [Configuration section](../README.md#configuration).

3. **Add V8 heap metrics** to `ActiveSupport::Notifications` — see [`plans/v8-heap-metrics.md`](v8-heap-metrics.md).

4. **Add a V8 heap size limit** — implemented via `SSR::Deno.max_heap_size_mb=` (default 64 MB). Passes `max_old_generation_size_in_bytes` via the Ruby → Rust bridge, capping V8 heap growth and preventing runaway memory from leaky components.

### 6.2 Medium Term

5. **Consider a dedicated SSR process pool** — a separate pool of Ruby processes (or a sidecar) that handle only SSR, fronted by a load balancer. This isolates SSR failures from the main Rails app. See [`plans/ssr-process-pool.md`](ssr-process-pool.md).

6. **Evaluate streaming SSR** (React 19's `renderToPipeableStream`) — reduces TTFB by sending HTML in chunks. Requires `ActionController::Live` integration. See [`plans/streaming-ssr.md`](streaming-ssr.md).

### 6.3 Long Term

7. ✅ **Multiple V8 isolates** — Implemented. See [`deno_runtime_wrapper.rs`](../ext/ssr_deno/src/deno_runtime_wrapper.rs) (`IsolatePool`). Each render is dispatched to the next available isolate via round-robin. Memory and throughput both scale linearly with pool size.

---

## 7. Summary

| Aspect | Verdict |
|---|---|
| **Memory overhead** | ~20–26 MB per Puma worker — acceptable for most Rails deployments |
| **Per-render latency** | ~5–50 ms — competitive with Node.js SSR |
| **Throughput bottleneck** | Single V8 isolate caps at ~20–200 req/s per worker |
| **Scaling strategy** | Scale Puma workers (processes), not threads |
| **Risk: hung renders** | No timeout — worker blocks indefinitely |
| **Risk: GC pauses** | V8 GC can cause latency spikes under high throughput |
| **Risk: memory leaks** | Shared V8 context means leaky components affect all renders |
