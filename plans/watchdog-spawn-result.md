# watchdog.rs — `expect` on thread spawn can panic

Status: Pending

## Problem

`Watchdog::spawn` calls `.expect("failed to spawn watchdog thread")`
when `std::thread::Builder::spawn` fails. OS thread creation can fail
under memory pressure or process limits. When it does, the panic
unwinds through the worker thread, corrupting that isolate pool slot.

## Implementation Draft

### Step 1 — change `Watchdog::spawn` to return `Result`

In `watchdog.rs`:

```rust
pub(super) fn spawn(
    v8_handle: v8::IsolateHandle,
    timeout_ms: u64,
    timeout_triggered: Arc<AtomicBool>,
) -> Result<Self, String> {
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
        .map_err(|e| format!("failed to spawn watchdog thread: {e}"))?;

    Ok(Self {
        cancel_tx: Some(cancel_tx),
        handle: Some(handle),
    })
}
```

### Step 2 — propagate error through `begin_render`

In `render.rs`, map the error to `SSRDenoError::Render` (not `WorkerInit` —
this is a per-render failure, not a pool init failure):

```rust
let watchdog = Watchdog::spawn(v8_handle, render_timeout_ms, timeout_triggered.clone())
    .map_err(|e| SSRDenoError::Render(e))?;
```

## Test Strategy

Not testable — thread spawn failure is impossible to trigger deterministically
in Ruby. The change is mechanical and correct by inspection. Verified by
code review + existing tests passing as regression coverage.

## Verification

- [x] Change `Watchdog::spawn` to return `Result`
- [x] Propagate error in `begin_render` via `SSRDenoError::Render`
- [x] `bundle exec rake` — must exit 0
