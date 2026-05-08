use std::sync::mpsc;

use tokio::sync::oneshot;

use super::types::{ChunkedRenderResult, WorkerMsg};
use super::SSRDenoError;

// ---------------------------------------------------------------------------
// IsolateHandle — per-isolate channel sender
// ---------------------------------------------------------------------------

/// Owns the channel to a dedicated background thread that runs a single
/// Deno `MainWorker` (V8 isolate + Web API extensions).
///
/// Because `MainWorker` never leaves its thread, no `unsafe` impl or
/// `UnsafeCell` is required — `tokio::sync::mpsc::Sender` is `Send + Sync`
/// on its own.
pub struct IsolateHandle {
    tx: tokio::sync::mpsc::Sender<WorkerMsg>,
    render_timeout_ms: u64,
}

impl IsolateHandle {
    /// Spawns a Deno worker thread with the given index and heap limit.
    /// Blocks until the worker is ready to accept messages.
    pub fn spawn(
        index: usize,
        max_heap_size_mb: usize,
        render_timeout_ms: u64,
        node_builtins: bool,
    ) -> Result<Self, SSRDenoError> {
        let (tx, rx) = tokio::sync::mpsc::channel::<WorkerMsg>(1);
        let (init_tx, init_rx) = mpsc::sync_channel::<Result<(), String>>(1);

        std::thread::Builder::new()
            .name(format!("deno-worker-{index}"))
            .spawn(move || {
                super::worker::worker_thread_main(rx, init_tx, max_heap_size_mb, node_builtins)
            })
            .map_err(|e| {
                SSRDenoError::WorkerInit(format!("Failed to spawn isolate thread {index}: {e}"))
            })?;

        init_rx
            .recv()
            .map_err(|_| {
                SSRDenoError::WorkerInit("Isolate thread exited unexpectedly during init".into())
            })?
            .map_err(SSRDenoError::WorkerInit)?;

        Ok(Self {
            tx,
            render_timeout_ms,
        })
    }

    /// Sends a render request to this isolate's worker thread and blocks
    /// until the result arrives. Runs the full Deno event loop (macrotasks,
    /// timers, I/O all fire). Returns the result as a JSON string.
    pub fn block_on_render(
        &self,
        bundle_id: &str,
        args_json: &str,
    ) -> Result<String, SSRDenoError> {
        let (reply_tx, reply_rx) = oneshot::channel::<Result<String, SSRDenoError>>();

        self.tx
            .blocking_send(WorkerMsg::Render {
                bundle_id: bundle_id.to_string(),
                args_json: args_json.to_string(),
                render_timeout_ms: self.render_timeout_ms,
                reply: reply_tx,
            })
            .map_err(|_| SSRDenoError::WorkerDied("Deno worker thread has exited".into()))?;

        reply_rx.blocking_recv().map_err(|_| {
            SSRDenoError::WorkerDied("Deno worker thread exited before reply".into())
        })?
    }

    /// Sends a chunked render request. Returns the chunk receiver
    /// immediately — the caller iterates it to get chunks as they arrive.
    /// The reply channel signals completion (Ok) or error (Err) after EOS.
    pub fn start_render_chunked(
        &self,
        bundle_id: &str,
        args_json: &str,
    ) -> Result<ChunkedRenderResult, SSRDenoError> {
        let (reply_tx, reply_rx) = oneshot::channel::<Result<(), SSRDenoError>>();
        let (chunk_tx, chunk_rx) = tokio::sync::mpsc::channel::<String>(64);

        self.tx
            .blocking_send(WorkerMsg::RenderChunked {
                bundle_id: bundle_id.to_string(),
                args_json: args_json.to_string(),
                render_timeout_ms: self.render_timeout_ms,
                chunk_tx,
                reply: reply_tx,
            })
            .map_err(|_| SSRDenoError::WorkerDied("Deno worker thread has exited".into()))?;

        Ok((chunk_rx, reply_rx))
    }

    /// Queries V8 heap statistics from this isolate's thread.
    pub fn block_on_heap_stats(&self) -> Result<String, SSRDenoError> {
        let (reply_tx, reply_rx) = oneshot::channel();

        self.tx
            .blocking_send(WorkerMsg::HeapStats { reply: reply_tx })
            .map_err(|_| SSRDenoError::WorkerDied("Deno worker thread has exited".into()))?;

        reply_rx.blocking_recv().map_err(|_| {
            SSRDenoError::WorkerDied("Deno worker thread exited before sending a reply".into())
        })?
    }

    /// Low-level send of a WorkerMsg. Used by IsolatePool for bundle broadcast.
    pub(crate) fn blocking_send(&self, msg: WorkerMsg) -> Result<(), SSRDenoError> {
        self.tx
            .blocking_send(msg)
            .map_err(|_| SSRDenoError::WorkerDied("Isolate worker has exited".into()))
    }
}
