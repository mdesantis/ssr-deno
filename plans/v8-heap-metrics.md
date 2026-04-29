# V8 Heap Metrics Instrumentation

> **Source:** Recommendation #3 from [`memory-performance-analysis.md`](memory-performance-analysis.md)
> **Cross-refs:** [`architecture.md`](../plans/architecture.md) (instrumentation via `ActiveSupport::Notifications`), [`instrumenter.rb`](../lib/ssr/deno/instrumenter.rb), [`railtie.rb`](../lib/ssr/deno/rails/railtie.rb) (existing event subscribers)

---

## Problem

Operations teams have no visibility into V8 memory pressure. A memory-leaking component, growing bundle size, or V8 heap fragmentation can silently increase RSS until OOM. The existing `ActiveSupport::Notifications` instrumentation emits `render.ssr_deno` and `bundle_load.ssr_deno` events, but none carry memory metrics.

## Approach

Add a new `WorkerMsg::HeapStats` variant that queries V8 `HeapStatistics` from the isolate and returns them as JSON. Expose this via a new `SSR::Deno.native_heap_stats` Ruby method. In the Railtie, add a subscriber that samples heap stats periodically (every N renders) and emits a `heap_stats.ssr_deno` event.

### Why a new message variant instead of piggybacking on render

- Heap stats are useful on their own (e.g., monitoring polling, health checks)
- Adding heap stats to every render response would change the `render` return type, complicating the hot path
- A separate message keeps concerns cleanly separated

### Sampling strategy

Heap stats are relatively expensive to collect (V8 must pause to gather statistics). Emitting them on every render would add unnecessary overhead. Instead, sample every 100th render by default, configurable via `config.ssr_deno.heap_stats_sample_rate`.

---

## Changes

### 1. [`ext/ssr_deno/src/deno_runtime_wrapper.rs`](../ext/ssr_deno/src/deno_runtime_wrapper.rs)

**Add `HeapStats` variant to `WorkerMsg`:**

```rust
enum WorkerMsg {
    LoadBundle { /* ... existing ... */ },
    Render { /* ... existing ... */ },
    HeapStats {
        reply: tokio::sync::oneshot::Sender<Result<String, DenoError>>,
    },
}
```

**Add `heap_stats` method to `DenoRuntimeWrapper`:**

```rust
pub fn heap_stats(&self) -> Result<String, DenoError> {
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();

    self.tx
        .blocking_send(WorkerMsg::HeapStats { reply: reply_tx })
        .map_err(|_| DenoError::WorkerDied("Deno worker thread has exited".into()))?;

    reply_rx
        .blocking_recv()
        .map_err(|_| DenoError::WorkerDied("Deno worker thread exited before sending a reply".into()))?
}
```

**Add handler in `worker_thread_main`:**

```rust
WorkerMsg::HeapStats { reply } => {
    let js_runtime = &mut worker.js_runtime;
    let isolate = js_runtime.v8_isolate();
    let mut stats = v8::HeapStatistics::default();
    isolate.get_heap_statistics(&mut stats);

    let stats_json = serde_json::json!({
        "total_heap_size": stats.total_heap_size(),
        "total_heap_size_executable": stats.total_heap_size_executable(),
        "total_physical_size": stats.total_physical_size(),
        "total_available_size": stats.total_available_size(),
        "used_heap_size": stats.used_heap_size(),
        "heap_size_limit": stats.heap_size_limit(),
        "malloced_memory": stats.malloced_memory(),
        "external_memory": stats.external_memory(),
        "peak_malloced_memory": stats.peak_malloced_memory(),
        "number_of_native_contexts": stats.number_of_native_contexts(),
        "number_of_detached_contexts": stats.number_of_detached_contexts(),
        "total_global_handles_size": stats.total_global_handles_size(),
        "used_global_handles_size": stats.used_global_handles_size(),
    });

    let _ = reply.send(Ok(stats_json.to_string()));
}
```

### 2. [`ext/ssr_deno/src/lib.rs`](../ext/ssr_deno/src/lib.rs)

**Add `native_heap_stats` function:**

```rust
fn native_heap_stats() -> Result<String, Error> {
    get_runtime()?
        .heap_stats()
        .map_err(|e| js_runtime_worker_error(e.to_string()))
}
```

**Register as singleton method in `init`:**

```rust
deno_module.define_singleton_method("native_heap_stats", function!(native_heap_stats, 0))?;
```

### 3. [`lib/ssr/deno.rb`](../lib/ssr/deno.rb)

**Add `heap_stats` class method (optional convenience wrapper):**

```ruby
module SSR
  module Deno
    class << self
      # Returns V8 heap statistics as a Hash.
      # Keys: total_heap_size, used_heap_size, heap_size_limit, etc.
      # @return [Hash<String, Integer>]
      def heap_stats
        JSON.parse(native_heap_stats)
      end
    end
  end
end
```

### 4. [`lib/ssr/deno/rails/railtie.rb`](../lib/ssr/deno/rails/railtie.rb)

**Add config option and sampled subscriber:**

```ruby
config.ssr_deno.heap_stats_sample_rate = 100 # emit heap stats every N renders

initializer 'ssr_deno.heap_stats', after: 'ssr_deno.subscribe_events' do |_app|
  sample_rate = config.ssr_deno.heap_stats_sample_rate
  counter = 0
  mutex = Mutex.new

  ActiveSupport::Notifications.subscribe('render.ssr_deno') do |*args|
    should_sample = false

    mutex.synchronize do
      counter += 1
      should_sample = (counter % sample_rate == 0)
    end

    next unless should_sample

    stats = SSR::Deno.heap_stats
    ActiveSupport::Notifications.instrument('heap_stats.ssr_deno', stats)
  rescue SSR::Deno::Error => e
    Rails.logger.warn "[ssr-deno] Failed to collect heap stats: #{e.message}"
  end
end
```

### 5. [`sig/ssr/deno.rbs`](../sig/ssr/deno.rbs)

**Add type signature:**

```rbs
module SSR
  module Deno
    def self.heap_stats: () -> Hash[String, Integer]
    def self.native_heap_stats: () -> String
  end
end
```

---

## Testing

### Rust unit test

Verify that `heap_stats` returns a valid JSON string with expected keys:

```rust
#[test]
fn test_heap_stats_returns_valid_json() {
    let wrapper = DenoRuntimeWrapper::new().unwrap();
    let stats_json = wrapper.heap_stats().unwrap();
    let stats: serde_json::Value = serde_json::from_str(&stats_json).unwrap();
    assert!(stats.get("total_heap_size").is_some());
    assert!(stats.get("used_heap_size").is_some());
    assert!(stats.get("heap_size_limit").is_some());
}
```

### Ruby unit test — [`test/ssr/test_deno.rb`](../test/ssr/test_deno.rb)

```ruby
def test_heap_stats
  # Ensure runtime is initialized first
  SSR::Deno::Bundle.new(BUNDLE_PATH)

  stats = SSR::Deno.heap_stats
  assert_kind_of Hash, stats
  assert stats.key?('total_heap_size')
  assert stats.key?('used_heap_size')
  assert stats.key?('heap_size_limit')
  assert stats['total_heap_size'].is_a?(Integer)
  assert stats['total_heap_size'] > 0
end
```

### Rails integration test — [`test/ssr/integration_deno_rails.rb`](../test/ssr/integration_deno_rails.rb)

```ruby
def test_heap_stats_event_fires
  events = []
  callback = ->(name, *) { events << name }

  ActiveSupport::Notifications.subscribed(callback, /\.ssr_deno$/) do
    # Trigger sample_rate renders
    (Rails.application.config.ssr_deno.heap_stats_sample_rate + 1).times do
      @view.ssr_render({ page: 'home' })
    rescue SSR::Deno::BundleNotFoundError
      # Expected — no bundle registered in dummy app
    end
  end

  assert_includes events, 'heap_stats.ssr_deno'
end
```

---

## Implementation Order

1. Add `HeapStats` variant to `WorkerMsg` in [`deno_runtime_wrapper.rs`](../ext/ssr_deno/src/deno_runtime_wrapper.rs)
2. Add `heap_stats` method to `DenoRuntimeWrapper`
3. Add handler in `worker_thread_main`
4. Add `native_heap_stats` function in [`lib.rs`](../ext/ssr_deno/src/lib.rs)
5. Add `SSR::Deno.heap_stats` Ruby wrapper in [`deno.rb`](../lib/ssr/deno.rb)
6. Add config + sampled subscriber in [`railtie.rb`](../lib/ssr/deno/rails/railtie.rb)
7. Update RBS signatures in [`deno.rbs`](../sig/ssr/deno.rbs)
8. Add tests
9. Run `bundle exec rake` to verify full pipeline
