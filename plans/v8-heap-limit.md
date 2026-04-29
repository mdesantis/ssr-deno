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

Pass a configured `v8::CreateParams` to `WorkerOptions.create_params` in [`build_worker`](../ext/ssr_deno/src/deno_runtime_wrapper.rs:262), setting `max_old_generation_size_in_bytes` to a configurable limit. Expose the limit via an environment variable (`SSR_DENO_MAX_HEAP_SIZE_MB`) and optionally a Ruby-side config.

### Why this approach

- **Zero new dependencies** — `CreateParams` is already part of the `v8` crate, and `WorkerOptions.create_params` is already wired in `deno_runtime`
- **Minimal diff** — one field change in `build_worker`, one env var read
- **V8-native** — the limit is enforced by V8's GC, not a separate watchdog
- **Composes with heap metrics** — `HeapStatistics::heap_size_limit` will report the configured value, giving operators visibility into whether the limit is being approached

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

**Default if unset:** 0 (unlimited, current behavior) — preserve backward compatibility.

---

## Changes

### 1. [`ext/ssr_deno/src/deno_runtime_wrapper.rs`](../ext/ssr_deno/src/deno_runtime_wrapper.rs)

**In `build_worker`, replace `create_params: None` with a configured `CreateParams`:**

```rust
fn build_worker(main_module: &Url) -> Result<MainWorker, String> {
    // Read max heap size from environment (MB), default 0 = unlimited
    let max_heap_mb: usize = std::env::var("SSR_DENO_MAX_HEAP_SIZE_MB")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    let create_params = if max_heap_mb > 0 {
        let limit_bytes = max_heap_mb * 1024 * 1024;
        Some(
            v8::CreateParams::default()
                .max_old_generation_size_in_bytes(limit_bytes),
        )
    } else {
        None
    };

    let options = WorkerOptions {
        create_params,
        // ... rest unchanged
    };

    // ... rest of function unchanged
}
```

**Alternative (simpler inline):**

```rust
let max_heap = std::env::var("SSR_DENO_MAX_HEAP_SIZE_MB")
    .ok().and_then(|v| v.parse::<usize>().ok())
    .map(|mb| v8::CreateParams::default()
        .max_old_generation_size_in_bytes(mb * 1024 * 1024));

let options = WorkerOptions {
    create_params: max_heap,
    // ...
};
```

### 2. No changes needed elsewhere

- [`lib.rs`](../ext/ssr_deno/src/lib.rs) — no new Ruby method needed; env var is read at worker initialization time
- [`deno.rb`](../lib/ssr/deno.rb) — optional: add a Ruby accessor for documentation purposes
- [`railtie.rb`](../lib/ssr/deno/rails/railtie.rb) — optional: add `config.ssr_deno.max_heap_size_mb`

### 3. Optional: Ruby-side config in [`railtie.rb`](../lib/ssr/deno/rails/railtie.rb)

```ruby
config.ssr_deno.max_heap_size_mb = 64  # nil = use env var or default (unlimited)
```

This would require passing the value from Ruby to Rust at initialization time, which is more complex (requires changing the `native_load_bundle` or adding an init method). **Recommendation:** Start with env var only. Add Ruby config if users request it.

### 4. Optional: `NearHeapLimitCallback`

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

### Manual test

```bash
# Set a very low limit (8 MB)
SSR_DENO_MAX_HEAP_SIZE_MB=8 bundle exec ruby -e "
  require 'ssr/deno'
  bundle = SSR::Deno::Bundle.new('samples/vite-ssr-app/dist/server/entry-server.js')
  puts bundle.render({data: {message: 'Hello'}})
"
# Expected: V8 OOM crash or render succeeds if 8 MB is enough
```

### Ruby unit test — [`test/ssr/test_deno.rb`](../test/ssr/test_deno.rb)

```ruby
def test_heap_size_limit_env_var
  # Can't easily test in-process (env var read at worker init, OnceLock)
  # Instead, verify the env var parsing logic
  # This is better tested at the Rust level
end
```

### Rust unit test

```rust
#[test]
fn test_create_params_with_limit() {
    std::env::set_var("SSR_DENO_MAX_HEAP_SIZE_MB", "64");
    // Re-init would require resetting OnceLock — difficult
    // Instead, test the parsing logic directly
    let max_heap_mb: usize = std::env::var("SSR_DENO_MAX_HEAP_SIZE_MB")
        .ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    assert_eq!(max_heap_mb, 64);
}
```

### Integration test

```ruby
def test_render_with_heap_limit
  # Start a separate Ruby process with the env var set
  output = `SSR_DENO_MAX_HEAP_SIZE_MB=64 bundle exec ruby -e "
    require 'ssr/deno'
    b = SSR::Deno::Bundle.new('samples/vite-ssr-app/dist/server/entry-server.js')
    puts b.render({data: {message: 'test'}})
  "`
  assert $?.success?
  assert output.include?('<div')
end
```

---

## Implementation Order

1. Modify `build_worker` in [`deno_runtime_wrapper.rs`](../ext/ssr_deno/src/deno_runtime_wrapper.rs) to read `SSR_DENO_MAX_HEAP_SIZE_MB` and pass `create_params`
2. Add Rust unit test for env var parsing
3. Add Ruby integration test (subprocess with env var)
4. Run `bundle exec rake` to verify full pipeline
5. (Optional) Add Ruby-side config in [`railtie.rb`](../lib/ssr/deno/rails/railtie.rb)

---

## Open Questions

1. **Should we also set `max_young_generation_size`?** The young generation default is typically ~2–8 MB and scales with the old generation limit. For SSR workloads (short-lived render objects), a smaller young gen means more frequent but faster Scavenge GC. **Recommendation:** Leave at default for now.

2. **Should the limit be configurable per-isolate (for the multiple-isolates plan)?** Yes — when [`multiple-isolates.md`](multiple-isolates.md) is implemented, each isolate should get `total_limit / num_isolates`. This ensures predictable total memory.

3. **Should we expose the limit via `HeapStatistics`?** Already done — `heap_size_limit` in [`v8-heap-metrics.md`](v8-heap-metrics.md) reports the configured limit. No extra work needed.
