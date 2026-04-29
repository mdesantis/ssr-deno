# Multiple V8 Isolates

> **Source:** Recommendation #6 from [`memory-performance-analysis.md`](memory-performance-analysis.md)
> **Cross-refs:** [`architecture.md`](architecture.md) (single V8 isolate design), [`deno_runtime_wrapper.rs`](../ext/ssr_deno/src/deno_runtime_wrapper.rs) (channel-based serialization), [`ssr-process-pool.md`](ssr-process-pool.md) (alternative: process-level parallelism)

---

## Problem

The current architecture has a **single V8 isolate** per Ruby process. All render requests — from any thread or Ractor — serialize through a tokio channel with buffer depth 1. SSR throughput is capped at `1 / renderToString_time` per Puma worker, regardless of thread count.

```
Puma Thread 1 ──┐
Puma Thread 2 ──┼──> [channel(1)] ──> V8 Isolate (single)
Puma Thread 3 ──┘
                ↑
          All serialize here
```

For a typical 25ms render, max throughput is ~40 req/s per worker. Adding threads doesn't help.

## Approach

Replace the single `DenoRuntimeWrapper` (one worker thread, one V8 isolate) with an **isolate pool**: N worker threads, each with its own V8 isolate, fronted by a load-balancing dispatcher. Render requests are dispatched to the next available isolate, allowing parallel SSR within a single Ruby process.

### Architecture

```
                    ┌──────────────────────────────┐
                    │     IsolatePoolDispatcher     │
                    │  (round-robin / least-loaded) │
                    └──┬──────┬──────┬──────┬──────┘
                       │      │      │      │
              ┌────────┘      │      │      └────────┐
              │               │      │               │
        ┌─────▼─────┐  ┌─────▼─────┐  ┌─────▼─────┐
        │ Isolate 1  │  │ Isolate 2  │  │ Isolate N  │
        │ ┌───────┐  │  │ ┌───────┐  │  │ ┌───────┐  │
        │ │V8     │  │  │ │V8     │  │  │ │V8     │  │
        │ │Isolate│  │  │ │Isolate│  │  │ │Isolate│  │
        │ └───────┘  │  │ └───────┘  │  │ └───────┘  │
        │ Worker 1   │  │ Worker 2   │  │ Worker N   │
        └────────────┘  └────────────┘  └────────────┘
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
    pub fn new(size: usize) -> Result<Self, DenoError> {
        let mut handles = Vec::with_capacity(size);
        for i in 0..size {
            let handle = spawn_isolate_thread(i)?;
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

### Bundle Loading — Broadcast to All Isolates

Each bundle must be loaded into **every** isolate, since isolates don't share memory. The `load_bundle` method broadcasts to all handles:

```rust
pub fn load_bundle(&self, bundle_id: &str, bundle_path: &str) -> Result<(), DenoError> {
    // Load once, get the code
    let bundle_code = std::fs::read_to_string(bundle_path)
        .map_err(|e| DenoError::BundleLoad(format!("Cannot read bundle: {e}")))?;

    // Broadcast to all isolates
    for handle in &self.handles {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        handle.tx
            .blocking_send(WorkerMsg::LoadBundle {
                bundle_id: bundle_id.to_string(),
                bundle_code: bundle_code.clone(),
                script_name: /* ... */,
                reply: reply_tx,
            })
            .map_err(|_| DenoError::WorkerDied("Isolate worker has exited".into()))?;

        reply_rx
            .blocking_recv()
            .map_err(|_| DenoError::WorkerDied("Isolate worker exited before reply".into()))?
            .map_err(DenoError::BundleLoad)?;
    }
    Ok(())
}
```

**Memory impact:** Each isolate compiles the bundle independently. A 200 KB source bundle becomes ~3 MB of V8 bytecode per isolate. With 4 isolates, that's ~12 MB for bundle code vs ~3 MB today.

### Worker Thread — Same as Today

Each worker thread is identical to the current single-worker design: a `current_thread` Tokio runtime + `LocalSet` + `MainWorker`. The only change is that each worker has its own `DenoRuntimeWrapper`-equivalent.

### Ruby API — Transparent

The Ruby API doesn't change. `SSR::Deno::Bundle#render` still calls `native_render`. The Rust side dispatches to the pool internally:

```ruby
# No change — same API
bundle.render({ page: 'home' })
```

The pool size is configured at initialization:

```ruby
# config/initializers/ssr_deno.rb
SSR::Deno.configure do |config|
  config.isolate_pool_size = 4  # Number of V8 isolates
end
```

### Pool Size Guidance

| CPU Cores | Recommended Pool Size | Memory (per worker) | SSR Throughput (25ms render) |
|---|---|---|---|
| 2 | 1–2 | ~20–40 MB | ~40–80 req/s |
| 4 | 2–4 | ~40–80 MB | ~80–160 req/s |
| 8 | 4–6 | ~80–120 MB | ~160–240 req/s |
| 16 | 6–8 | ~120–160 MB | ~240–320 req/s |

**Rule of thumb:** `min(CPU_cores - 1, 8)`. Leave one core for the Ruby thread and OS. Beyond 8 isolates, the process pool approach ([`ssr-process-pool.md`](ssr-process-pool.md)) is more memory-efficient since each process can have its own GC schedule.

### Comparison: Isolate Pool vs Process Pool

| Factor | Isolate Pool (this plan) | Process Pool ([separate plan](ssr-process-pool.md)) |
|---|---|---|
| **Parallelism** | Within one Ruby process | Across separate processes |
| **Memory** | ~20–26 MB per isolate | ~20–26 MB per process + socket overhead |
| **Bundle loading** | Broadcast to all isolates | Each process loads independently |
| **Failure isolation** | ❌ A crashing isolate can take down the Ruby process | ✅ SSR process crash doesn't affect Rails |
| **Latency** | ~1 µs channel send | ~10–50 µs socket I/O |
| **Complexity** | Medium — pool dispatcher in Rust | Higher — socket protocol, process management |
| **Scaling** | Limited by process memory | Can scale across hosts |
| **Best for** | Single-process deployments, moderate throughput | Multi-process, high throughput, fault isolation |

### Interaction with Render Timeout

The timeout implementation from [`render-timeout.md`](render-timeout.md) applies per-isolate. Each worker thread has its own `recv_timeout` on the reply channel. A timeout on one isolate doesn't affect others.

### Interaction with Heap Size Limit

The V8 heap size limit from [`v8-heap-limit.md`](v8-heap-limit.md) applies **per-isolate**. When the isolate pool is active, the configured `max_heap_size_mb` is divided equally among isolates to ensure predictable total memory:

```
total_limit = SSR::Deno.max_heap_size_mb  # e.g. 256 MB
pool_size   = isolate_pool_size            # e.g. 4
per_isolate = total_limit / pool_size      # = 64 MB each
```

This means:

- **Each isolate** gets `max_heap_size_mb / pool_size` as its `v8::CreateParams::set_max_old_generation_size_in_bytes`
- **Total V8 memory** across all isolates is bounded by `max_heap_size_mb` (plus overhead for bytecode, which is outside the old generation)
- **Operators** can reason: "This Puma worker uses at most X MB for SSR" regardless of pool size

If `max_heap_size_mb` is left at the default (64 MB), a 4-isolate pool would give each isolate 16 MB — which may be too tight for complex pages. **Recommendation:** When configuring an isolate pool, increase `max_heap_size_mb` proportionally:

| Pool Size | Recommended `max_heap_size_mb` | Per-Isolate Budget |
|-----------|-------------------------------|--------------------|
| 1         | 64 MB (default)               | 64 MB              |
| 2         | 128 MB                        | 64 MB              |
| 4         | 256 MB                        | 64 MB              |
| 8         | 512 MB                        | 64 MB              |

The division is implemented in `IsolatePool::new`:

```rust
pub fn new(size: usize, total_heap_mb: usize) -> Result<Self, DenoError> {
    let per_isolate_mb = if size > 0 {
        std::cmp::max(1, total_heap_mb / size)  // at least 1 MB per isolate
    } else {
        total_heap_mb
    };

    let mut handles = Vec::with_capacity(size);
    for i in 0..size {
        let handle = spawn_isolate_thread(i, per_isolate_mb)?;
        handles.push(handle);
    }
    Ok(Self {
        handles,
        counter: AtomicUsize::new(0),
    })
}
```

### Interaction with Heap Metrics

The heap metrics from [`v8-heap-metrics.md`](v8-heap-metrics.md) need to be extended to report per-isolate stats:

```rust
pub fn heap_stats_all(&self) -> Result<Vec<HashMap<String, u64>>, DenoError> {
    self.handles.iter().map(|handle| {
        // Send HeapStats to each isolate
        // Collect results
    }).collect()
}
```

### Edge Cases

| Scenario | Behavior |
|---|---|
| **One isolate crashes** | The dispatcher removes the dead handle from the pool. Remaining isolates continue serving. `WorkerDied` error is returned for the failed render. |
| **All isolates busy** | The caller blocks on `blocking_send` (channel buffer = 1 per isolate). This is the same behavior as today, but with N parallel slots instead of 1. |
| **Bundle reload** | Broadcast reload to all isolates. If one fails, the reload is retried. |
| **Isolate count change at runtime** | Not supported initially. Pool size is fixed at initialization. A future enhancement could support dynamic resize. |

---

## Changes

### 1. [`ext/ssr_deno/src/deno_runtime_wrapper.rs`](../ext/ssr_deno/src/deno_runtime_wrapper.rs)

- Refactor `DenoRuntimeWrapper` into two parts:
  - `IsolateHandle` — the per-isolate channel sender (what `DenoRuntimeWrapper` is today)
  - `IsolatePool` — the dispatcher that owns multiple `IsolateHandle`s
- Keep `WorkerMsg`, `DenoError`, `worker_thread_main`, `build_worker`, `call_render`, `load_bundle_in_worker` as-is (they're per-isolate)

### 2. [`ext/ssr_deno/src/lib.rs`](../ext/ssr_deno/src/lib.rs)

- Replace `RUNTIME: OnceLock<DenoRuntimeWrapper>` with `POOL: OnceLock<IsolatePool>`
- Add pool size configuration (environment variable or Ruby-side config)
- `get_or_init_runtime` becomes `get_or_init_pool` with pool size parameter

### 3. [`lib/ssr/deno.rb`](../lib/ssr/deno.rb) — Optional

Add a configuration accessor:

```ruby
module SSR
  module Deno
    class << self
      attr_accessor :isolate_pool_size
    end
  end
end
```

### 4. [`lib/ssr/deno/rails/railtie.rb`](../lib/ssr/deno/rails/railtie.rb)

Add config option:

```ruby
config.ssr_deno.isolate_pool_size = nil  # nil = auto-detect (CPU cores - 1)
```

### 5. [`sig/ssr/deno.rbs`](../sig/ssr/deno.rbs)

Update type signatures.

---

## Testing

### Rust unit test

```rust
#[test]
fn test_isolate_pool_round_robin() {
    let pool = IsolatePool::new(3).unwrap();

    // Dispatch 6 renders, verify they're distributed across isolates
    let handles: Vec<_> = (0..6).map(|i| {
        std::thread::spawn(move || {
            pool.dispatch_render("test_bundle", "{}")
        })
    }).collect();

    for handle in handles {
        assert!(handle.join().unwrap().is_ok());
    }
}

#[test]
fn test_isolate_pool_bundle_broadcast() {
    let pool = IsolatePool::new(2).unwrap();
    pool.load_bundle("test", "path/to/bundle.js").unwrap();

    // Both isolates should have the bundle loaded
    let result1 = pool.dispatch_render("test", "{}").unwrap();
    let result2 = pool.dispatch_render("test", "{}").unwrap();
    assert!(result1.contains("<div"));
    assert!(result2.contains("<div"));
}

#[test]
fn test_isolate_crash_removed_from_pool() {
    let pool = IsolatePool::new(2).unwrap();
    // Force-crash one isolate (implementation detail)
    // Verify remaining isolate still works
    // Verify pool size is reduced
}
```

### Ruby unit test — [`test/ssr/test_deno_concurrency.rb`](../test/ssr/test_deno_concurrency.rb)

```ruby
def test_parallel_renders_with_isolate_pool
  bundle = SSR::Deno::Bundle.new(BUNDLE_PATH)

  threads = 4.times.map do
    Thread.new do
      bundle.render({ data: { message: 'Hello' } })
    end
  end

  results = threads.map(&:value)
  assert_equal 4, results.length
  results.each { |r| assert r.include?('<div') }
end

def test_isolate_pool_config
  SSR::Deno.isolate_pool_size = 2
  bundle = SSR::Deno::Bundle.new(BUNDLE_PATH)
  assert bundle.render({ data: { message: 'Hello' } }).include?('<div')
end
```

### Benchmark — [`test/ssr/bench_isolate_pool.rb`](../test/ssr/bench_isolate_pool.rb)

```ruby
def bench_throughput
  bundle = SSR::Deno::Bundle.new(BUNDLE_PATH)

  # Warmup
  bundle.render({ data: { message: 'warmup' } })

  # Benchmark with 1, 2, 4, 8 threads
  [1, 2, 4, 8].each do |thread_count|
    start = Time.now
    count = 100

    threads = thread_count.times.map do
      Thread.new do
        (count / thread_count).times do
          bundle.render({ data: { message: 'Hello' } })
        end
      end
    end
    threads.each(&:join)

    elapsed = Time.now - start
    puts "#{thread_count} threads: #{count / elapsed.round(2)} req/s"
  end
end
```

---

## Implementation Order

1. Refactor `DenoRuntimeWrapper` into `IsolateHandle` + `IsolatePool` in [`deno_runtime_wrapper.rs`](../ext/ssr_deno/src/deno_runtime_wrapper.rs)
2. Implement `IsolatePool::new(size)` — spawn N worker threads
3. Implement `IsolatePool::dispatch_render` — round-robin dispatch
4. Implement `IsolatePool::load_bundle` — broadcast to all isolates
5. Update [`lib.rs`](../ext/ssr_deno/src/lib.rs) — replace `RUNTIME` with `POOL`, add pool size config
6. Add Ruby-side config in [`deno.rb`](../lib/ssr/deno.rb) and [`railtie.rb`](../lib/ssr/deno/rails/railtie.rb)
7. Add tests
8. Update RBS signatures
9. Run `bundle exec rake` to verify full pipeline

---

## Open Questions

1. **Should we use round-robin or least-loaded dispatch?** Round-robin is simpler and sufficient for uniform render times. Least-loaded (tracking pending renders per isolate) would help if render times vary significantly. **Recommendation:** Start with round-robin, add least-loaded as a future optimization.

2. **Should bundle loading be parallelized across isolates?** Currently the plan broadcasts sequentially. Parallel loading (spawning one task per isolate) would reduce initialization time but adds complexity. For 2–8 isolates and a ~200 KB bundle, sequential loading takes ~10–40 ms total — acceptable.

3. **How to handle the `OnceLock` migration?** Currently `RUNTIME` is a `OnceLock<DenoRuntimeWrapper>`. The pool needs to be initialized with a size parameter. Options:
   - Use a config struct set before first access
   - Use environment variable (`SSR_DENO_ISOLATE_POOL_SIZE`)
   - Make `get_or_init_runtime` accept a size parameter
   **Recommendation:** Environment variable with Ruby-side override, since `OnceLock` can only be set once.

4. **Should we add a `max_isolates` cap?** Yes — prevent accidental over-allocation on high-core-count machines. Default cap of 8.

5. **Does the Ractor-safe declaration still hold?** Yes — `IsolatePool` dispatches through channels, which are `Send + Sync`. Each Ractor gets its own result without shared mutable state.
