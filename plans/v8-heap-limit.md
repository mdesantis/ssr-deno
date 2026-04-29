# V8 Heap Size Limit

> **Source:** Discovered during review of [`memory-performance-analysis.md`](memory-performance-analysis.md) — V8's `CreateParams` exposes heap size constraints that are already wired through `WorkerOptions.create_params` but currently set to `None`.
> **Cross-refs:** [`deno_runtime_wrapper.rs`](../ext/ssr_deno/src/deno_runtime_wrapper.rs) (build_worker, line 289), [`v8-heap-metrics.md`](v8-heap-metrics.md) (HeapStatistics reporting), [`multiple-isolates.md`](multiple-isolates.md) (per-isolate memory budget)

---

## Problem

The V8 isolate has no explicit memory cap. V8's default `max_old_generation_size` is based on available system memory — typically **~1.4 GB on 64-bit systems** — which is far more than ssr-deno needs (~20–50 MB for typical SSR workloads).

Without a cap:

1. **A memory-leaking component** can grow the V8 heap unchecked until the OS OOM-kills the Ruby process, taking down the entire Puma worker.

2. **No predictable memory budget** — operators can't say "each Puma worker uses at most X MB for SSR." This makes capacity planning harder, especially with multiple isolates ([`multiple-isolates.md`](multiple-isolates.md)).

3. **V8 GC pressure increases** with heap size — larger heaps mean longer mark-sweep pauses when GC eventually runs.

## Approach

Pass a configured `v8::CreateParams` to `WorkerOptions.create_params` in [`build_worker`](../ext/ssr_deno/src/deno_runtime_wrapper.rs:262), setting `max_old_generation_size_in_bytes` to a configurable limit.

The config flows through a **Ruby → Rust bridge** — no env var reading in Rust:

```
Ruby: SSR::Deno.native_set_max_heap_size_mb(64)
        │
        ▼
Rust:  static CONFIG: OnceLock<Config>   ← stores the value
        │
        ▼
       DenoRuntimeWrapper::new(max_heap_size_mb)
        │
        ▼
       worker_thread_main(rx, init_tx, max_heap_size_mb)
        │
        ▼
       build_worker(&main_module_url, max_heap_size_mb)
        │
        ▼
       WorkerOptions { create_params: Some(v8::CreateParams::default()
           .set_max_old_generation_size_in_bytes(64 * 1024 * 1024)) }
```

### Why this approach

- **Zero new dependencies** — `CreateParams` is already part of the `v8` crate, and `WorkerOptions.create_params` is already wired in `deno_runtime`
- **Clean separation** — Ruby owns configuration, Rust owns execution. No env var reading in native code.
- **V8-native** — the limit is enforced by V8's GC, not a separate watchdog
- **Composes with heap metrics** — `HeapStatistics::heap_size_limit` will report the configured value, giving operators visibility into whether the limit is being approached
- **Extensible** — the `Config` struct can hold future configuration values without adding more env var reads or more Ruby methods

### How V8 enforces the limit

When the old generation approaches `max_old_generation_size`, V8:

1. Triggers incremental marking (GC prep)
2. If memory continues growing, triggers a full mark-sweep GC
3. If still over the limit after GC, invokes `NearHeapLimitCallback` (if registered)
4. If the callback returns 0 or isn't registered, V8 **crashes the isolate** with OOM

This means the limit is a **hard cap** — V8 will not exceed it. The process will crash if the limit is too low for the workload. Choose a limit that provides headroom above the expected peak.

### Recommended default

| Workload | Recommended Limit | Rationale |
|---|---|---|
| Single bundle, simple pages | 64 MB | ~20 MB baseline + 2–5 MB bundle + 5 MB render peak + headroom |
| Single bundle, complex pages | 128 MB | ~20 MB baseline + 5 MB bundle + 20 MB render peak + headroom |
| Multiple bundles (2–3) | 128–256 MB | ~20 MB baseline + 10–15 MB bundles + headroom |
| Multiple isolates (4×) | 64 MB per isolate | 256 MB total for 4 isolates |

**Default if unset:** 64 MB — sensible for typical SSR workloads (~20 MB baseline + headroom). Can be overridden via `SSR::Deno.max_heap_size_mb = 0` for unlimited (V8 default, ~1.4 GB) or any other value.

---

## Changes

### 1. [`ext/ssr_deno/src/lib.rs`](../ext/ssr_deno/src/lib.rs) — Add `Config` struct and `native_set_max_heap_size_mb`

Add a static config that Ruby writes to before runtime initialization:

```rust
/// Configuration passed from Ruby to Rust before runtime initialization.
/// All fields have safe defaults so the runtime can be initialized without
/// calling any setter.
#[derive(Clone, Copy)]
struct Config {
    max_heap_size_mb: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self { max_heap_size_mb: 64 } // 64 MB — sensible for SSR workloads
    }
}

static CONFIG: OnceLock<Config> = OnceLock::new();
```

Add a Ruby-callable function to set the config, with overflow-safe validation:

```rust
/// Called by Ruby before the first Bundle.new to configure the V8 heap limit.
/// Must be called before any native_load_bundle or native_render call.
///
/// Validates that the value doesn't overflow when converted to bytes.
/// The max safe value is usize::MAX / 1024 / 1024 (~16 TB on 64-bit),
/// which is far beyond any practical V8 heap limit.
fn native_set_max_heap_size_mb(mb: usize) -> Result<(), Error> {
    // Check that mb * 1024 * 1024 doesn't overflow usize.
    // On 64-bit: max ≈ 16,384,000 MB (16 TB). On 32-bit: max ≈ 4,096 MB.
    mb.checked_mul(1024 * 1024)
        .ok_or_else(|| {
            Error::new(
                Ruby::get().unwrap().exception_arg_error(),
                format!(
                    "max_heap_size_mb={mb} overflows when converted to bytes (max: {})",
                    usize::MAX / 1024 / 1024
                ),
            )
        })?;

    CONFIG
        .set(Config {
            max_heap_size_mb: mb,
        })
        .map_err(|_| {
            Error::new(
                deno_exc("JsRuntimeInitializationError"),
                "Cannot set config after runtime is already initialized",
            )
        })
}
```

Register it in the `init` function alongside the other native methods:

```rust
deno_module.define_singleton_method(
    "native_set_max_heap_size_mb",
    function!(native_set_max_heap_size_mb, 1),
)?;
```

Update `get_or_init_runtime` to pass the config value to `DenoRuntimeWrapper::new`:

```rust
fn get_or_init_runtime() -> Result<&'static DenoRuntimeWrapper, Error> {
    if let Some(r) = RUNTIME.get() {
        return Ok(r);
    }
    let _guard = INIT_LOCK.lock().unwrap();
    if let Some(r) = RUNTIME.get() {
        return Ok(r);
    }
    let config = CONFIG.get().copied().unwrap_or_default();
    let rt = DenoRuntimeWrapper::new(config.max_heap_size_mb)
        .map_err(|e| js_runtime_initialization_error(e.to_string()))?;
    let _ = RUNTIME.set(rt);
    Ok(RUNTIME.get().unwrap())
}
```

### 2. [`ext/ssr_deno/src/deno_runtime_wrapper.rs`](../ext/ssr_deno/src/deno_runtime_wrapper.rs) — Thread config through to `build_worker`

**`DenoRuntimeWrapper::new`** accepts `max_heap_size_mb`:

```rust
pub fn new(max_heap_size_mb: usize) -> Result<Self, DenoError> {
    let (tx, rx) = tokio::sync::mpsc::channel::<WorkerMsg>(1);
    let (init_tx, init_rx) = mpsc::sync_channel::<Result<(), String>>(1);

    std::thread::Builder::new()
        .name("deno-worker".into())
        .spawn(move || worker_thread_main(rx, init_tx, max_heap_size_mb))
        .map_err(|e| DenoError::WorkerInit(format!("Failed to spawn worker thread: {e}")))?;

    init_rx
        .recv()
        .map_err(|_| DenoError::WorkerInit("Deno worker thread exited unexpectedly during init".into()))?
        .map_err(DenoError::WorkerInit)?;

    Ok(Self { tx })
}
```

**`worker_thread_main`** accepts and forwards `max_heap_size_mb`:

```rust
fn worker_thread_main(
    mut rx: tokio::sync::mpsc::Receiver<WorkerMsg>,
    init_tx: mpsc::SyncSender<Result<(), String>>,
    max_heap_size_mb: usize,
) {
    // ... tokio runtime setup unchanged ...

    let mut worker = match build_worker(&main_module_url, max_heap_size_mb) {
        Ok(w) => w,
        Err(e) => {
            let _ = init_tx.send(Err(e));
            return;
        }
    };

    // ... rest unchanged ...
}
```

**`build_worker`** accepts `max_heap_size_mb` and uses it for `create_params` — **no `std::env::var` call**:

```rust
fn build_worker(main_module: &Url, max_heap_size_mb: usize) -> Result<MainWorker, String> {
    let create_params = if max_heap_size_mb > 0 {
        Some(
            v8::CreateParams::default()
                .set_max_old_generation_size_in_bytes(max_heap_size_mb * 1024 * 1024),
        )
    } else {
        None
    };

    let options = WorkerOptions {
        create_params,
        // ... rest unchanged
    };

    // ... rest unchanged ...
}
```

### 3. [`lib/ssr/deno.rb`](../lib/ssr/deno.rb) — Add Ruby accessor

```ruby
module SSR
  module Deno
    class << self
      # Set the maximum V8 heap size in megabytes before initializing the runtime.
      # Must be called before any Bundle.new call.
      # @param mb [Integer] heap size in MB, or 0 for unlimited (V8 default)
      def max_heap_size_mb=(mb)
        native_set_max_heap_size_mb(mb.to_i)
      end
    end
  end
end
```

### 4. [`lib/ssr/deno/rails/railtie.rb`](../lib/ssr/deno/rails/railtie.rb) — Rails config

```ruby
config.ssr_deno.max_heap_size_mb = nil  # nil = 64 MB (default)
```

In `init_bundles`, before any `Bundle.new`:

```ruby
initializer 'ssr_deno.init_bundles', after: :load_config_initializers do |_app|
  next unless config.ssr_deno.enabled

  # Apply V8 heap size limit before runtime initialization
  if config.ssr_deno.max_heap_size_mb
    SSR::Deno.max_heap_size_mb = config.ssr_deno.max_heap_size_mb
  end

  config.ssr_deno.bundles.each do |name, path|
    # ... rest unchanged
  end
end
```

### 3. Optional: `NearHeapLimitCallback`

For extra safety, register a callback that logs a warning before the OOM crash:

```rust
// Unsafe extern C callback
unsafe extern "C" fn near_heap_limit_callback(
    data: *mut c_void,
    current_limit: usize,
    initial_limit: usize,
) -> usize {
    // Log warning (can't use stdlib easily from extern C)
    // Return 0 to let V8 crash, or return a higher limit to give more headroom
    current_limit + (initial_limit / 10) // Allow 10% over cap
}
```

This is optional and adds `unsafe` code. **Not recommended for v1** — start with the hard cap only.

---

## Testing

### Ruby unit test — [`test/ssr/test_deno.rb`](../test/ssr/test_deno.rb)

```ruby
def test_set_max_heap_size_mb
  # Must be called before any Bundle.new (OnceLock)
  SSR::Deno.max_heap_size_mb = 64
  # Verify it was stored (no error raised)
  assert true
end

def test_set_max_heap_size_mb_after_init_raises
  SSR::Deno::Bundle.new('samples/vite-ssr-app/dist/server/entry-server.js')
  assert_raises(SSR::Deno::JsRuntimeInitializationError) do
    SSR::Deno.max_heap_size_mb = 64
  end
end
```

### Integration test

```ruby
def test_render_with_heap_limit
  # Start a separate Ruby process — config via Ruby API, not env var
  output = `bundle exec ruby -e "
    require 'ssr/deno'
    SSR::Deno.max_heap_size_mb = 64
    b = SSR::Deno::Bundle.new('samples/vite-ssr-app/dist/server/entry-server.js')
    puts b.render({data: {message: 'test'}})
  "`
  assert $?.success?
  assert output.include?('<div')
end
```

### Manual test (low limit to trigger OOM)

```bash
bundle exec ruby -e "
  require 'ssr/deno'
  SSR::Deno.max_heap_size_mb = 8
  bundle = SSR::Deno::Bundle.new('samples/vite-ssr-app/dist/server/entry-server.js')
  puts bundle.render({data: {message: 'Hello'}})
"
# Expected: V8 OOM crash or render succeeds if 8 MB is enough
```

---

## Implementation Order

1. ✅ Modify `build_worker` in [`deno_runtime_wrapper.rs`](../ext/ssr_deno/src/deno_runtime_wrapper.rs) to accept `max_heap_size_mb` parameter — **Done**
2. ✅ Remove `std::env::var` call from `build_worker` — Rust no longer reads env vars directly — **Done**
3. ✅ Add `Config` struct and `native_set_max_heap_size_mb` to [`lib.rs`](../ext/ssr_deno/src/lib.rs) — **Done**
4. ✅ Thread `max_heap_size_mb` through `DenoRuntimeWrapper::new` → `worker_thread_main` → `build_worker` — **Done**
5. ✅ Add `SSR::Deno.max_heap_size_mb=` accessor in [`deno.rb`](../lib/ssr/deno.rb) — **Done**
6. ✅ Add `config.ssr_deno.max_heap_size_mb` in [`railtie.rb`](../lib/ssr/deno/rails/railtie.rb) — **Done**
7. ✅ Add Ruby unit tests — **Done**
8. ✅ Run `bundle exec rake` to verify full pipeline — **Done**

---

## Open Questions

1. **Should we also set `max_young_generation_size`?** The young generation default is typically ~2–8 MB and scales with the old generation limit. For SSR workloads (short-lived render objects), a smaller young gen means more frequent but faster Scavenge GC. **Recommendation:** Leave at default for now.

2. **Should the limit be configurable per-isolate (for the multiple-isolates plan)?** Yes — when [`multiple-isolates.md`](multiple-isolates.md) is implemented, each isolate should get `total_limit / num_isolates`. This ensures predictable total memory.

3. **Should we expose the limit via `HeapStatistics`?** Already done — `heap_size_limit` in [`v8-heap-metrics.md`](v8-heap-metrics.md) reports the configured limit. No extra work needed.
