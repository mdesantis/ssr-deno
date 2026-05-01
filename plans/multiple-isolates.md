# Multiple V8 Isolates `[IMPLEMENTED]`

> **Source:** Recommendation #6 from [`memory-performance-analysis.md`](memory-performance-analysis.md)
> **Cross-refs:** [`architecture.md`](architecture.md) (single V8 isolate design), [`deno_runtime_wrapper.rs`](../ext/ssr_deno/src/deno_runtime_wrapper.rs) (channel-based serialization), [`ssr-process-pool.md`](ssr-process-pool.md) (alternative: process-level parallelism), [`lib.rs`](../ext/ssr_deno/src/lib.rs) (pool init)
> **Status:** ✅ All 9 implementation steps completed. 36/36 tests passing, 100% coverage.

---

## Problem

The current architecture has a **single V8 isolate** per Ruby process. All render requests -- from any thread or Ractor -- serialize through a tokio channel with buffer depth 1. SSR throughput is capped at `1 / renderToString_time` per Puma worker, regardless of thread count.

```
Puma Thread 1 --,
Puma Thread 2 --+--> [channel(1)] --> V8 Isolate (single)
Puma Thread 3 --'
                ^
          All serialize here
```

For a typical 25ms render, max throughput is ~40 req/s per worker. Adding threads doesn't help.

## Approach

Replace the single `DenoRuntimeWrapper` (one worker thread, one V8 isolate) with an **isolate pool**: N worker threads, each with its own V8 isolate, fronted by a load-balancing dispatcher. Render requests are dispatched to the next available isolate, allowing parallel SSR within a single Ruby process.

### Architecture

```
                    +------------------------------+
                    |     IsolatePoolDispatcher     |
                    |  (round-robin, atomic counter) |
                    +--+------+------+------+------+
                       |      |      |      |
              +--------'      |      |      '--------+
              |               |      |               |
        +-----v-----+  +-----v-----+  +-----v-----+
        | Isolate 1  |  | Isolate 2  |  | Isolate N  |
        | +-------+  |  | +-------+  |  | +-------+  |
        | |V8     |  |  | |V8     |  |  | |V8     |  |
        | |Isolate|  |  | |Isolate|  |  | |Isolate|  |
        | +-------+  |  | +-------+  |  | +-------+  |
        | Worker 0   |  | Worker 1   |  | Worker N-1 |
        +------------+  +------------+  +------------+
```

### Dispatcher Design

The dispatcher owns a vector of `IsolateHandle` structs, each wrapping a `tokio::sync::mpsc::Sender<WorkerMsg>` to a dedicated worker thread:

```rust
pub struct IsolatePool {
    handles: Vec<IsolateHandle>,
    counter: AtomicUsize,  // For round-robin
}

struct IsolateHandle {
    tx: tokio::sync::mpsc::Sender<WorkerMsg>,
    // Future: track load (pending renders per isolate)
}
```

**Dispatch strategy (round-robin):**

```rust
impl IsolatePool {
    pub fn new(size: usize, per_isolate_heap_mb: usize) -> Result<Self, DenoError> {
        if size == 0 {
            return Err(DenoError::WorkerInit(
                "Pool size must be at least 1".into()
            ));
        }
        let mut handles = Vec::with_capacity(size);
        for i in 0..size {
            let handle = spawn_isolate_thread(i, per_isolate_heap_mb)?;
            handles.push(handle);
        }
        Ok(Self {
            handles,
            counter: AtomicUsize::new(0),
        })
    }

    fn next_handle(&self) -> &IsolateHandle {
        let idx = self.counter.fetch_add(1, Ordering::Relaxed) % self.handles.len();
        &self.handles[idx]
    }

    pub fn dispatch_render(
        &self,
        bundle_id: &str,
        args_json: &str,
    ) -> Result<String, DenoError> {
        let handle = self.next_handle();
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();

        handle.tx
            .blocking_send(WorkerMsg::Render {
                bundle_id: bundle_id.to_string(),
                args_json: args_json.to_string(),
                reply: reply_tx,
            })
            .map_err(|_| DenoError::WorkerDied("Isolate worker has exited".into()))?;

        reply_rx
            .blocking_recv()
            .map_err(|_| DenoError::WorkerDied("Isolate worker exited before reply".into()))?
    }
}
```

### Spawning Isolate Threads

Each isolate thread is created with a unique name for debugging, and receives its own per-isolate heap budget:

```rust
fn spawn_isolate_thread(
    index: usize,
    per_isolate_heap_mb: usize,
) -> Result<IsolateHandle, DenoError> {
    let (tx, rx) = tokio::sync::mpsc::channel::<WorkerMsg>(1);
    let (init_tx, init_rx) = std::sync::mpsc::sync_channel::<Result<(), String>>(1);

    std::thread::Builder::new()
        .name(format!("deno-worker-{index}"))
        .spawn(move || worker_thread_main(rx, init_tx, per_isolate_heap_mb))
        .map_err(|e| DenoError::WorkerInit(
            format!("Failed to spawn isolate thread {index}: {e}")
        ))?;

    init_rx
        .recv()
        .map_err(|_| DenoError::WorkerInit(
            "Isolate thread exited unexpectedly during init".into()
        ))?
        .map_err(DenoError::WorkerInit)?;

    Ok(IsolateHandle { tx })
}
```

The `worker_thread_main` function is **identical** to today's single-worker design: a `current_thread` Tokio runtime + `LocalSet` + `MainWorker`. The only change is the thread name and the per-isolate heap limit.

### Bundle Loading -- Broadcast to All Isolates

Each bundle must be loaded into **every** isolate, since isolates don't share memory. The `load_bundle` method broadcasts to all handles:

```rust
pub fn load_bundle(&self, bundle_id: &str, bundle_path: &str) -> Result<(), DenoError> {
    // Resolve path and read bundle code once (same as today)
    let canonical = std::fs::canonicalize(bundle_path)
        .map_err(|e| DenoError::BundleLoad(format!("Cannot resolve bundle path: {e}")))?;
    let bundle_code = std::fs::read_to_string(bundle_path)
        .map_err(|e| DenoError::BundleLoad(format!("Cannot read bundle: {e}")))?;

    // MainWorker::execute_script requires &'static str for script name.
    // One bounded leak per bundle load (shared by all isolates).
    let script_name: &'static str = canonical
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| Box::leak(s.to_owned().into_boxed_str()) as &'static str)
        .unwrap_or("main.js");

    // Broadcast to all isolates (sequential -- see note below)
    for (i, handle) in self.handles.iter().enumerate() {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        handle.tx
            .blocking_send(WorkerMsg::LoadBundle {
                bundle_id: bundle_id.to_string(),
                bundle_code: bundle_code.clone(),
                script_name,
                reply: reply_tx,
            })
            .map_err(|_| DenoError::WorkerDied(
                "Isolate worker has exited".into()
            ))?;

        reply_rx
            .blocking_recv()
            .map_err(|_| DenoError::WorkerDied(
                "Isolate worker exited before reply".into()
            ))?
            .map_err(DenoError::BundleLoad)?;
    }
    Ok(())
}
```

**Memory impact:** Each isolate compiles the bundle independently. A 200 KB source bundle becomes ~3 MB of V8 bytecode per isolate. With 4 isolates, that's ~12 MB for bundle code vs ~3 MB today.

**Performance note:** The broadcast is sequential. For 4 isolates and a 200 KB bundle, total load time is ~20-40 ms. For 8 isolates with a complex 500 KB bundle, it could reach ~160 ms. This is acceptable for initialization, but parallel loading (spawning one task per isolate) could be added as a future optimization if bundle load time becomes a concern during hot reloads.

### Worker Thread -- Same as Today

Each worker thread is identical to the current single-worker design: a `current_thread` Tokio runtime + `LocalSet` + `MainWorker`. The only differences are:
- Thread name: `"deno-worker-{index}"` instead of `"deno-worker"`
- Heap limit: `per_isolate_heap_mb` instead of the global `max_heap_size_mb`

### Ruby API -- Transparent

The Ruby API doesn't change. `SSR::Deno::Bundle#render` still calls `native_render`. The Rust side dispatches to the pool internally:

```ruby
# No change -- same API
bundle.render({ page: 'home' })
```

The pool size is configured at initialization, following the same `CONFIG: OnceLock<Config>` pattern already established for `max_heap_size_mb`:

```ruby
# config/initializers/ssr_deno.rb
SSR::Deno.configure do |config|
  config.isolate_pool_size = 4  # Number of V8 isolates, nil = auto-detect
end
```

### Configuration Pathway

The pool size uses a **`Mutex<Config>` with an `INITIALIZED: OnceLock<()>` guard** (refined from the original `OnceLock<Config>` approach). A plain `OnceLock<Config>` only supports one `.set()` call, which breaks when two config fields (`max_heap_size_mb` + `isolate_pool_size`) need to be set independently. The `Mutex` allows each setter to modify its own field; the `INITIALIZED` guard prevents modification after pool init.

```
Ruby: SSR::Deno.isolate_pool_size = 4
        |
        v
Rust:  native_set_isolate_pool_size(4)
        |  CONFIG.lock().unwrap().isolate_pool_size = 4;
        |  guarded by: check_not_initialized()
        v
       get_or_init_pool() reads CONFIG, resolves pool size,
       creates IsolatePool::new(size, per_isolate_mb)
```

This is consistent with the pattern established in [`v8-heap-limit.md`](v8-heap-limit.md) -- Ruby owns configuration, Rust owns execution.

### Auto-Detection of Pool Size

When `isolate_pool_size` is left at the default (nil/0), the pool size is auto-detected:

```
pool_size = min(available_parallelism - 1, MAX_ISOLATES)
           where available_parallelism = std::thread::available_parallelism()
           and MAX_ISOLATES = 8
```

This happens in `get_or_init_pool()` on the Rust side:

```rust
fn get_or_init_pool() -> Result<&'static IsolatePool, Error> {
    if let Some(p) = POOL.get() { return Ok(p); }
    let _guard = INIT_LOCK.lock().unwrap();
    if let Some(p) = POOL.get() { return Ok(p); }

    let config = CONFIG.get().copied().unwrap_or_default();

    // Auto-detect pool size if not explicitly configured
    let pool_size = if config.isolate_pool_size > 0 {
        config.isolate_pool_size
    } else {
        let cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(2);
        std::cmp::max(1, std::cmp::min(cores.saturating_sub(1), MAX_ISOLATES))
    };

    // Per-isolate heap: each isolate gets the full max_heap_size_mb.
    // max_heap_size_mb is a per-isolate V8 CreateParams constraint, NOT a
    // total budget. Dividing it would starve each isolate on auto-detected
    // pools (e.g. 64 MB / 8 = 8 MB → V8 OOM).
    let per_isolate_mb = config.max_heap_size_mb;

    let pool = IsolatePool::new(pool_size, per_isolate_mb)
        .map_err(|e| js_runtime_initialization_error(e.to_string()))?;
    let _ = POOL.set(pool);
    Ok(POOL.get().unwrap())
}
```

### Pool Size Guidance

| CPU Cores | Recommended Pool Size | Memory (per worker, idle) | SSR Throughput (25ms render) |
|---|---|---|---|
| 2 | 1-2 | ~20-52 MB | ~40-80 req/s |
| 4 | 2-4 | ~40-104 MB | ~80-160 req/s |
| 8 | 4-7 | ~80-182 MB | ~160-280 req/s |
| 16 | 7-8 | ~140-208 MB | ~280-320 req/s |

**Memory notes:**
- Idle per-isolate memory: ~15-26 MB (V8 baseline + Deno runtime + thread stack)
- Bundle bytecode: ~2-5 MB per isolate per bundle
- Peak render transient: ~0.5-5 MB per render (freed by V8 GC)
- **Total per-isolate (1 bundle, idle): ~20-26 MB**

**Rule of thumb:** `min(CPU_cores - 1, 8)`. Leave one core for the Ruby thread and OS. Beyond 8 isolates, the process pool approach ([`ssr-process-pool.md`](ssr-process-pool.md)) is more memory-efficient since each process can have its own GC schedule.

### MAX_ISOLATES Cap

A hard cap of **8 isolates** prevents accidental over-allocation on high-core-count machines:

```rust
const MAX_ISOLATES: usize = 8;
```

### Comparison: Isolate Pool vs Process Pool

| Factor | Isolate Pool (this plan) | Process Pool ([separate plan](ssr-process-pool.md)) |
|---|---|---|
| **Parallelism** | Within one Ruby process | Across separate processes |
| **Memory** | ~20-26 MB per isolate | ~20-26 MB per process + socket overhead |
| **Bundle loading** | Broadcast to all isolates | Each process loads independently |
| **Failure isolation** | x A crashing isolate can take down the Ruby process | / SSR process crash doesn't affect Rails |
| **Latency** | ~1 us channel send | ~10-50 us socket I/O |
| **Complexity** | Medium -- pool dispatcher in Rust | Higher -- socket protocol, process management |
| **Scaling** | Limited by process memory | Can scale across hosts |
| **Best for** | Single-process deployments, moderate throughput | Multi-process, high throughput, fault isolation |

### Interaction with Render Timeout

The timeout implementation from [`render-timeout.md`](render-timeout.md) applies per-isolate. Each worker thread has its own `recv_timeout` on the reply channel. A timeout on one isolate doesn't affect others.

### Interaction with Heap Size Limit

The V8 heap size limit from [`v8-heap-limit.md`](v8-heap-limit.md) applies **per-isolate**. Each isolate independently gets the configured `max_heap_size_mb` as its V8 heap cap. The budget is **not divided** because `max_heap_size_mb` is a per-isolate `v8::CreateParams` constraint, not a total process budget.

**Why no division:** If `max_heap_size_mb = 64` and the pool auto-detects 8 isolates on a 24-core machine, dividing would give each isolate only 8 MB — causing V8 to OOM on bundle loading. The 64 MB default was calibrated for a single-isolate workload and must apply fully to each isolate.

### Interaction with Heap Metrics

The heap metrics from [`v8-heap-metrics.md`](v8-heap-metrics.md) need to be extended to report per-isolate stats. The return type should be a JSON string (matching the existing `heap_stats` convention):

```rust
pub fn heap_stats_all(&self) -> Result<String, DenoError> {
    let mut results = Vec::with_capacity(self.handles.len());
    for handle in &self.handles {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        handle.tx
            .blocking_send(WorkerMsg::HeapStats { reply: reply_tx })
            .map_err(|_| DenoError::WorkerDied("Isolate worker has exited".into()))?;
        let stats_json = reply_rx
            .blocking_recv()
            .map_err(|_| DenoError::WorkerDied("Isolate worker exited before reply".into()))??;
        results.push(stats_json);
    }
    // Return JSON array of per-isolate stats
    Ok(format!("[{}]", results.join(",")))
}
```

### Ractor Safety

The existing `rb_ext_ractor_safe(true)` declaration still holds with the pool:

- `IsolatePool` owns a `Vec<IsolateHandle>`; each handle wraps a `tokio::sync::mpsc::Sender<WorkerMsg>` which is `Send + Sync`
- The round-robin counter uses `AtomicUsize`, which is lock-free and `Sync`
- Each render call sends data through a channel -- no shared mutable state
- Each Ractor gets its own result without interference

### Edge Cases

| Scenario | Behavior |
|---|---|
| **One isolate crashes** | The dispatcher removes the dead handle from the pool. Remaining isolates continue serving. `WorkerDied` error is returned for the failed render. *Note: v1 does not implement removal — crash of any isolate returns WorkerDied for that render.* |
| **All isolates busy** | The caller blocks on `blocking_send` (channel buffer = 1 per isolate). This is the same behavior as today, but with N parallel slots instead of 1. |
| **Bundle reload** | Broadcast reload to all isolates. If one fails, the reload is retried. |
| **Isolate count change at runtime** | Not supported initially. Pool size is fixed at initialization. |
| **Pool size = 0** | Guarded in `IsolatePool::new` — returns `Err(DenoError::WorkerInit(...))` |
| **Pool size exceeds MAX_ISOLATES** | Capped at `MAX_ISOLATES = 8` in `get_or_init_pool` |

---

## Changes Made

### 1. ✅ [`ext/ssr_deno/src/deno_runtime_wrapper.rs`](../ext/ssr_deno/src/deno_runtime_wrapper.rs)

- Refactored `DenoRuntimeWrapper` into `IsolateHandle` (per-isolate channel sender) + `IsolatePool` (dispatcher)
- `IsolateHandle::spawn(index, max_heap_size_mb)` replaces `DenoRuntimeWrapper::new(max_heap_size_mb)` — thread name is `deno-worker-{index}`
- `IsolatePool::new(size, per_isolate_heap_mb)` spawns N handles, validates size (1–8)
- `IsolatePool::dispatch_render()` round-robins via `AtomicUsize::fetch_add(1) % N`
- `IsolatePool::load_bundle()` resolves path once (canonicalize, symlink check, read), broadcasts to all handles sequentially
- `size()` method with `#[allow(dead_code)]` for future use
- Internal functions (`worker_thread_main`, `load_bundle_in_worker`, `build_worker`, `call_render`) unchanged
- `MAX_ISOLATES = 8` hard cap
- `heap_stats_all` NOT implemented (future work — see heap metrics plan)

### 2. ✅ [`ext/ssr_deno/src/lib.rs`](../ext/ssr_deno/src/lib.rs)

- `CONFIG` storage changed from `OnceLock<Config>` to `Mutex<Config>` with eager defaults + `INITIALIZED: OnceLock<()>` guard
  - Rationale: `OnceLock` only supports one `.set()` call, breaking multiple config fields
- Added `check_not_initialized()` helper used by both setters
- Added `isolate_pool_size: usize` to `Config` (default `0` = auto-detect)
- Added `native_set_isolate_pool_size(n)` function, registered in `init()`
- Replaced `RUNTIME` with `POOL: OnceLock<IsolatePool>`
- Added `resolve_pool_size()` with auto-detect: `available_parallelism() - 1`, capped at 8
- Added `get_or_init_pool()` (replaces `get_or_init_runtime`)
  - Reads config, resolves pool size, passes full `max_heap_size_mb` per-isolate (NOT divided)
- Added `get_pool()` (replaces `get_runtime`)
- Updated `native_load_bundle` → `get_or_init_pool()?.load_bundle(...)`
- Updated `native_render` → `get_pool()?.dispatch_render(...)`

### 3. ✅ [`lib/ssr/deno.rb`](../lib/ssr/deno.rb)

```ruby
def isolate_pool_size=(size)
  native_set_isolate_pool_size(size.to_i)
end
```

### 4. ✅ [`lib/ssr/deno/rails/railtie.rb`](../lib/ssr/deno/rails/railtie.rb)

```ruby
config.ssr_deno.isolate_pool_size = nil  # nil = auto-detect
# In init_bundles initializer:
SSR::Deno.isolate_pool_size = config.ssr_deno.isolate_pool_size if config.ssr_deno.isolate_pool_size
```

### 5. ✅ [`sig/ssr/deno.rbs`](../sig/ssr/deno.rbs)

```rbs
def self.native_set_isolate_pool_size: (Integer size) -> nil
def self.isolate_pool_size=: (Integer size) -> void
```

### 6. ✅ [`test/ssr/test_deno.rb`](../test/ssr/test_deno.rb)

Added `test_set_isolate_pool_size` (same rescue-`JsRuntimeInitializationError` pattern as `test_set_max_heap_size_mb`).

---

## Implementation Order (Checklist)

- [x] **1.** Add `isolate_pool_size` to `Config` + `native_set_isolate_pool_size`
- [x] **2.** Refactor `deno_runtime_wrapper.rs`: `IsolateHandle` + `IsolatePool`
- [x] **3.** `IsolateHandle::spawn(index, heap_mb)` with named threads
- [x] **4.** Replace `RUNTIME` with `POOL`, implement `get_or_init_pool` with auto-detect
- [x] **5.** Update `native_load_bundle` and `native_render` to use pool methods
- [x] **6.** Add Ruby-side config in `deno.rb` and `railtie.rb`
- [x] **7.** Add tests (Ruby unit: `test_set_isolate_pool_size`)
- [x] **8.** Update RBS signatures in `deno.rbs`
- [x] **9.** `bundle exec rake` — 36/36 pass, 100% coverage

## Deviations from Original Plan

| Plan | Actual | Reason |
|------|--------|--------|
| `CONFIG: OnceLock<Config>` | `CONFIG: Mutex<Config>` + `INITIALIZED: OnceLock<()>` | `OnceLock` only allows one `.set()` — breaks multiple config fields |
| Divide `max_heap_size_mb / pool_size` | Pass full `max_heap_size_mb` per-isolate | Division starves each isolate (64/8=8 MB → V8 OOM); `max_heap_size_mb` is a per-isolate constraint, not a total budget |
| `heap_stats_all` method | Not implemented | Deferred to heap metrics plan |
| Separate `spawn_isolate_thread` function | Inline in `IsolateHandle::spawn` | Simpler, no separate function needed |
| `test_isolate_pool_parallel_renders` with `pool_size = 2` | Existing concurrency tests cover this | Pool always active even with default config |
| `isolate_pool_size=` accepts `nil` | Accepts `Integer` only, passes through to native | `nil` guard is in the railtie, not the accessor |

---

## Open Questions (Resolved)

1. **Round-robin vs least-loaded dispatch?** Round-robin is simpler and sufficient for uniform render times. **Start with round-robin.**

2. **Parallel bundle loading?** Sequential broadcast is simpler. Parallel loading can be added later if hot-reload time becomes a concern. **Keep sequential for v1.**

3. **How to pass pool size to `OnceLock`?** Use the existing `CONFIG: OnceLock<Config>` pattern (not env vars). Add `isolate_pool_size: usize` to `Config`. **Consistent with established pattern.**

4. **MAX_ISOLATES cap?** Yes -- `const MAX_ISOLATES: usize = 8`. **Applied in `get_or_init_pool`.**

5. **Ractor-safe?** Yes -- `IsolatePool` dispatches through channels, which are `Send + Sync`. The `AtomicUsize` counter is also `Sync`. **Existing `rb_ext_ractor_safe(true)` still holds.**
