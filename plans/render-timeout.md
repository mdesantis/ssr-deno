# Render Timeout

> **Source:** Recommendation #1 from [`memory-performance-analysis.md`](memory-performance-analysis.md)
> **Cross-refs:** [`architecture.md`](architecture.md#error-handling-strategy) (notes "No timeout, retry, or bundle-reload behavior is implemented yet"), [`deno_runtime_wrapper.rs`](../ext/ssr_deno/src/deno_runtime_wrapper.rs)

---

## Problem

[`call_render`](../ext/ssr_deno/src/deno_runtime_wrapper.rs:316) invokes the JS `render` function synchronously on the V8 thread. If the function hangs (infinite loop, deadlock in a Promise, runaway recursion), the entire V8 isolate blocks forever. All subsequent SSR requests queue up on the tokio channel (buffer depth = 1) and eventually time out at the Rack/HTTP layer with no visibility into the cause.

## Approach

Change the `WorkerMsg::Render` reply channel from `tokio::sync::oneshot` to `std::sync::mpsc::SyncSender`/`Receiver`, then use `recv_timeout` in [`block_on_render`](../ext/ssr_deno/src/deno_runtime_wrapper.rs:154).

### Why this approach

- **No new threads** — the timeout is on the receiver side, not a separate watchdog
- **No async gymnastics** — `std::sync::mpsc::Receiver::recv_timeout` is a plain blocking call, which is what we're already doing with `blocking_recv`
- **Isolate remains usable** — when the timeout fires, the render function is still running in V8, but it will eventually finish and try to send on a closed channel (which fails silently). The worker thread continues processing subsequent messages
- **Minimal diff** — only the reply channel type changes; the worker thread's message loop stays the same

### What happens on timeout

1. Ruby thread calls `block_on_render` → sends `WorkerMsg::Render` → blocks on `reply_rx.recv_timeout(30s)`
2. Worker thread picks up the render → `call_render` hangs
3. After 30s, `recv_timeout` returns `Err(RecvTimeoutError::Timeout)`
4. `block_on_render` returns `Err(DenoError::Render("Render timed out after 30s"))`
5. Ruby side raises `SSR::Deno::RenderError`
6. Worker thread eventually finishes the hung render → tries `reply.send(result)` → channel is closed → send fails silently (error ignored)
7. Worker thread continues processing next messages — V8 isolate is healthy

### Edge case: worker death during timeout

If the worker thread panics or exits while Ruby is waiting on `recv_timeout`, the channel disconnects. `recv_timeout` returns `Err(RecvTimeoutError::Disconnected)`, which maps to `DenoError::WorkerDied`. This is the same behavior as the current `blocking_recv` error path.

---

## Changes

### 1. [`ext/ssr_deno/src/deno_runtime_wrapper.rs`](../ext/ssr_deno/src/deno_runtime_wrapper.rs)

**`WorkerMsg::Render` variant** — change `reply` type:

```rust
// Before:
Render {
    bundle_id: String,
    args_json: String,
    reply: tokio::sync::oneshot::Sender<Result<String, DenoError>>,
},

// After:
Render {
    bundle_id: String,
    args_json: String,
    reply: std::sync::mpsc::SyncSender<Result<String, DenoError>>,
},
```

**`block_on_render` method** — replace `blocking_recv` with `recv_timeout`:

```rust
use std::time::Duration;

pub fn block_on_render(&self, bundle_id: &str, args_json: &str) -> Result<String, DenoError> {
    let (reply_tx, reply_rx) = std::sync::mpsc::sync_channel::<Result<String, DenoError>>(1);

    self.tx
        .blocking_send(WorkerMsg::Render {
            bundle_id: bundle_id.to_string(),
            args_json: args_json.to_string(),
            reply: reply_tx,
        })
        .map_err(|_| DenoError::WorkerDied("Deno worker thread has exited".into()))?;

    match reply_rx.recv_timeout(Duration::from_secs(30)) {
        Ok(result) => result,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            Err(DenoError::Render("Render timed out after 30s".into()))
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            Err(DenoError::WorkerDied("Deno worker thread exited before sending a reply".into()))
        }
    }
}
```

**Worker thread handler** — no change needed. `SyncSender::send` works the same as `oneshot::Sender::send` for our use case:

```rust
WorkerMsg::Render { bundle_id, args_json, reply } => {
    let result = call_render(&mut worker, &bundle_id, &args_json);
    let _ = reply.send(result); // SyncSender, but same API
}
```

### 2. No changes needed elsewhere

- [`lib.rs`](../ext/ssr_deno/src/lib.rs) — `DenoError::Render` already exists, no new error variant needed
- [`bundle.rb`](../lib/ssr/deno/bundle.rb) — `RenderError` already rescued in the helper; timeout error propagates as `RenderError` naturally
- [`helper.rb`](../lib/ssr/deno/rails/helper.rb) — `RenderError` already handled by the existing rescue block

---

## Testing

### Unit test (Rust)

Add a test bundle that contains an infinite loop:

```javascript
// Infinite loop bundle
globalThis.render = function() {
    while(true) {} // hangs forever
};
```

Test that `block_on_render` returns `Err(DenoError::Render(...))` within ~30s.

### Unit test (Ruby) — [`test/ssr/test_deno_errors.rb`](../test/ssr/test_deno_errors.rb)

```ruby
def test_render_timeout
  # Create a bundle with an infinite loop
  # Verify SSR::Deno::RenderError is raised
  # Verify the bundle is still usable after the timeout
end
```

### Recovery test

After a timeout, verify that a subsequent normal render succeeds:

```ruby
def test_render_works_after_timeout
  # First render hangs → timeout
  # Second render (normal data) → succeeds
end
```

---

## Implementation Order

1. Modify `WorkerMsg::Render` reply type in [`deno_runtime_wrapper.rs`](../ext/ssr_deno/src/deno_runtime_wrapper.rs)
2. Replace `blocking_recv` with `recv_timeout` in `block_on_render`
3. Add Rust unit test for timeout behavior
4. Add Ruby unit test for timeout + recovery
5. Run `bundle exec rake` to verify full pipeline
