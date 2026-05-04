# watchdog.rs — `expect` on thread spawn can panic

Status: Pending

## Problem

`Watchdog::spawn` calls `.expect("failed to spawn watchdog thread")`
when `std::thread::Builder::spawn` fails. OS thread creation can fail
under memory pressure or process limits. When it does, the panic
unwinds across the Ruby FFI boundary, crashing the Ruby process with
no recovery.

## Implementation Draft

### Step 1 — change `Watchdog::spawn` to return `Result`

In `watchdog.rs`:

```rust
pub(super) fn spawn(
    v8_handle: v8::IsolateHandle,
    timeout_ms: u64,
    timeout_triggered: Arc<AtomicBool>,
) -> Result<Self, &'static str> {
    let (cancel_tx, cancel_rx) = std::sync::mpsc::channel::<()>();

    let handle = std::thread::Builder::new()
        .name("ssr-watchdog".into())
        .spawn(move || {
            match cancel_rx.recv_timeout(Duration::from_millis(timeout_ms)) {
                Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    timeout_triggered.store(true, Ordering::SeqCst);
                    v8_handle.terminate_execution();
                }
            }
        })
        .map_err(|e| "failed to spawn watchdog thread")?;

    Ok(Self {
        cancel_tx: Some(cancel_tx),
        handle: Some(handle),
    })
}
```

### Step 2 — propagate error through `begin_render`

In `render.rs`, `begin_render` calls `Watchdog::spawn`. Change:

```rust
let watchdog = Watchdog::spawn(v8_handle, render_timeout_ms, timeout_triggered.clone())?;
```

Since `begin_render` already returns `Result<(Watchdog, Arc<AtomicBool>), SSRDenoError>`,
and `SSRDenoError` has a `WorkerInit` variant, map the error:

```rust
let watchdog = Watchdog::spawn(v8_handle, render_timeout_ms, timeout_triggered.clone())
    .map_err(|e| SSRDenoError::WorkerInit(e.to_string()))?;
```

This means if the watchdog thread can't be spawned, the render fails
with `JsRuntimeInitializationError` instead of panicking.

**Alternative: fallback to no watchdog.** If the thread can't be
created, run without timeout protection. This is more resilient but
risks infinite-blocking renders. Not recommended — better to fail
fast with a clear error.

### Step 3 — remove `expect` in `Drop`

Also in `watchdog.rs` `Drop`, the thread `join` also uses `_` discard:

```rust
impl Drop for Watchdog {
    fn drop(&mut self) {
        drop(self.cancel_tx.take());
        if let Some(h) = self.handle.take() {
            let _ = h.join();  // already discarding, no change needed
        }
    }
}
```

No change needed for `Drop` — `h.join()` already has its result
discarded.

## Test Strategy

Test via subprocess — verify that when thread creation fails, the
error is caught:

```ruby
def test_watchdog_spawn_failure_raises_worker_error
  assert_subprocess(<<~RUBY, 'Expected WorkerInit on thread failure')
    # Use a custom thread spawn limit to force failure
    # (platform-dependent; skip if ulimit can't be set)
    begin
      SSR::Deno::Bundle.new(MINIMAL_BUNDLE)
      exit 0
    rescue SSR::Deno::JsRuntimeInitializationError
      exit 0
    end
  RUBY
end
```

This test is hard to make deterministic (thread spawn failure under
normal conditions is rare). **Recommendation:** Manual review + code
audit instead of automated test. The change is mechanical (`.expect`
→ `Result::map_err`).

## Verification

- [ ] Change `Watchdog::spawn` to return `Result`
- [ ] Propagate error in `begin_render`
- [ ] `bundle exec rake` — must exit 0
