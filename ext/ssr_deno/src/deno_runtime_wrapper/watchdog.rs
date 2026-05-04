use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use deno_runtime::deno_core::v8;

// ---------------------------------------------------------------------------
// Watchdog — terminates V8 execution from a separate thread on timeout
// ---------------------------------------------------------------------------

/// A watchdog thread that calls `terminate_execution()` on a V8 isolate after
/// a deadline. The only way to interrupt synchronous JS code (e.g., infinite
/// `while` loops) is from another OS thread — the event loop cannot fire while
/// V8 is executing a single script frame.
///
/// The watchdog uses a channel-based wait for precise cancellation: dropping
/// the sender (or joining after cancel) wakes the thread immediately without
/// busy-polling.
pub(super) struct Watchdog {
    /// Dropping this sender signals the watchdog thread to exit without
    /// triggering termination. When the receiver's `recv_timeout` returns
    /// `Disconnected`, the watchdog knows the render completed normally.
    cancel_tx: Option<std::sync::mpsc::Sender<()>>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Watchdog {
    /// Spawns a watchdog thread. After `timeout_ms` elapses without
    /// cancellation, calls `terminate_execution()` on the V8 isolate and
    /// sets `timeout_triggered` to true.
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
                    // Cancelled or sender dropped before timeout — render completed.
                    Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {}
                    // Timeout expired — terminate JS execution.
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

    /// Cancels the watchdog (render completed in time). Drops the sender to
    /// wake the watchdog thread, then joins it to ensure cleanup.
    pub(super) fn cancel(mut self) {
        drop(self.cancel_tx.take());
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        // Safety net: if cancel() was not called explicitly (e.g., early
        // return via `?`), still signal the watchdog to avoid a dangling
        // thread.
        drop(self.cancel_tx.take());
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}
