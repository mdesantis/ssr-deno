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
- **Minimal diff** — only the reply channel type changes; the worker thread's message loop stays the same

> **Limitation — worker thread blocks until `call_render` returns.** The worker thread loop ([`deno_runtime_wrapper.rs:259`](../ext/ssr_deno/src/deno_runtime_wrapper.rs:259)) runs synchronously inside each message handler. A hung `call_render` prevents the worker from processing further messages until it completes. See [post-timeout isolate state](#post-timeout-isolate-state) below.

### What happens on timeout

1. Ruby thread calls `block_on_render` → sends `WorkerMsg::Render` → blocks on `reply_rx.recv_timeout(10s)`
2. Worker thread picks up the render → `call_render` hangs
3. After 10s, `recv_timeout` returns `Err(RecvTimeoutError::Timeout)`
4. `block_on_render` returns `Err(DenoError::Render("Render timed out after 10s"))`
5. Ruby side raises `SSR::Deno::RenderError`
6. Worker thread eventually finishes the hung render → tries `reply.send(result)` → channel is closed → send fails silently (error ignored)
7. Worker thread resumes processing subsequent messages

### Post-timeout isolate state

**The hung isolate is degraded until `call_render` returns.** Because the worker thread is single-threaded:

- **Infinite loop** (e.g. `while(true){}`): isolate is dead permanently. The worker thread never exits `call_render` and cannot process any further messages.
- **Finite but slow** (e.g. runaway recursion that hits stack limit): isolate recovers when `call_render` eventually returns (step 7 above).

**Pool-level impact**: With a pool of N isolates, a hang on one isolate reduces effective capacity to N-1. The Ruby side selects the next isolate via round-robin ([`next_handle`](../ext/ssr_deno/src/deno_runtime_wrapper.rs:146)), so subsequent renders still work if another isolate is available.

**Future enhancement**: `v8::V8::TerminateExecution` could forcibly abort the hung script, immediately restoring the isolate. This is more invasive (requires careful V8 scope state management) and is out of scope for this PR.

### Recovery test caveat

The Ruby recovery test (`test_render_works_after_timeout`) only passes reliably when `isolate_pool_size > 1`. With a single isolate, the second render dispatches to the same hung isolate and also times out. The test should explicitly configure a pool size ≥ 2, or run as part of the integration suite where the pool auto-detects multiple cores.

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

**`block_on_render` method** — replace `blocking_recv` with `recv_timeout`. Add a named constant at module level:

```rust
/// Maximum time to wait for a render response from the V8 isolate.
const RENDER_TIMEOUT: Duration = Duration::from_secs(10);

pub fn block_on_render(&self, bundle_id: &str, args_json: &str) -> Result<String, DenoError> {
    let (reply_tx, reply_rx) = std::sync::mpsc::sync_channel::<Result<String, DenoError>>(1);

    self.tx
        .blocking_send(WorkerMsg::Render {
            bundle_id: bundle_id.to_string(),
            args_json: args_json.to_string(),
            reply: reply_tx,
        })
        .map_err(|_| DenoError::WorkerDied("Deno worker thread has exited".into()))?;

    match reply_rx.recv_timeout(RENDER_TIMEOUT) {
        Ok(result) => result,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            Err(DenoError::Render(
                format!("Render timed out after {}s", RENDER_TIMEOUT.as_secs()),
            ))
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

### Rust unit test — deferred

`call_render` lives in [`deno_runtime_wrapper.rs`](../ext/ssr_deno/src/deno_runtime_wrapper.rs), which depends on `v8`, `deno_runtime`, and `tokio`. Adding `#[cfg(test)]` tests here requires building V8 for test runs (~30s compile overhead). The `ssr_deno_core` crate is zero-dep and is the appropriate home for pure-logic Rust tests.

For this PR, **skip the Rust unit test** and cover timeout behavior through Ruby integration tests. A Rust-level test can be added in a follow-up if the timeout logic grows in complexity (e.g. configurable timeout, `TerminateExecution`).

### Ruby integration test — [`test/ssr/test_deno_errors.rb`](../test/ssr/test_deno_errors.rb)

The test creates a temp JS file where `render` spins for 60s (bounded, not infinite). Bundle load succeeds (top-level eval is just `function` assignment); calling `render` triggers the 10s timeout.

> **Why bounded instead of `while(true)`**: if the timeout mechanism is broken, `while(true)` hangs the subprocess indefinitely, making the test runner hang. A >10s spin ensures the Rust timeout wins the race when working, while guaranteeing the subprocess always exits eventually (60s worst case instead of forever). Run as a subprocess via `Open3.capture3` for clean environment isolation.

```ruby
HANG_JS = "... Date.now() + 60000 spin ..."

def test_render_timeout
  script = <<~RUBY
    require 'tmpdir'
    ...
    File.write(bundle_path, #{HANG_JS.inspect})
    bundle = SSR::Deno::Bundle.new(bundle_path)
    begin
      bundle.render({})
      exit 1
    rescue SSR::Deno::RenderError
      exit 0
    end
  RUBY
  _, _, status = Open3.capture3(RbConfig.ruby, '-e', script, chdir: GEM_ROOT)
  assert_predicate status.exitstatus, :zero?
end
```

### Recovery test — requires `pool_size > 1`

After a timeout, the hung isolate is blocked until `call_render` returns (see [post-timeout isolate state](#post-timeout-isolate-state)). A subsequent render must dispatch to a **different isolate** in the pool. This test only passes when `isolate_pool_size >= 2`.

Uses the same bounded 60s spin JS for the first render (not `while(true)`) to guarantee subprocess termination even if timeout is broken.

```ruby
def test_render_works_after_timeout
  script = <<~RUBY
    ...
    SSR::Deno.isolate_pool_size = 2
    Dir.mktmpdir do |dir|
      File.write(hang_path, #{HANG_JS.inspect})
      hang_bundle = SSR::Deno::Bundle.new(hang_path)
      begin
        hang_bundle.render({})
      rescue SSR::Deno::RenderError
        # expected — hung isolate is now blocked
      end
      # Second render uses next isolate via round-robin
      File.write(ok_path, "globalThis.render = function() { return '<h1>ok</h1>'; };")
      ok_bundle = SSR::Deno::Bundle.new(ok_path)
      assert_equal '<h1>ok</h1>', ok_bundle.render({})
    end
  RUBY
end
```

---

## Implementation Order

1. [x] Add `const RENDER_TIMEOUT: Duration = Duration::from_secs(30)` at module level in [`deno_runtime_wrapper.rs`](../ext/ssr_deno/src/deno_runtime_wrapper.rs)
2. [x] Change `WorkerMsg::Render` reply type from `tokio::sync::oneshot::Sender` to `std::sync::mpsc::SyncSender`
3. [x] Replace `blocking_recv` with `recv_timeout(RENDER_TIMEOUT)` in `block_on_render`
4. [x] Add Ruby integration test `test_render_timeout` — infinite loop bundle → `RenderError`
5. [x] Add Ruby recovery test `test_render_works_after_timeout` — requires pool size ≥ 2
6. [x] Run `bundle exec rake` to verify full pipeline
