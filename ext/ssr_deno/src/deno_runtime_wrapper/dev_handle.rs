use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

use tokio::sync::oneshot;

use super::types::ChunkedRenderResult;
use super::SSRDenoError;

pub(crate) enum DevWorkerMsg {
    LoadEntry {
        entry_path: String,
        resolve_alias: HashMap<String, String>,
        reply: oneshot::Sender<Result<(), SSRDenoError>>,
    },
    Render {
        bundle_id: String,
        args_json: String,
        render_timeout_ms: u64,
        reply: oneshot::Sender<Result<String, SSRDenoError>>,
    },
    RenderChunked {
        bundle_id: String,
        args_json: String,
        render_timeout_ms: u64,
        chunk_tx: tokio::sync::mpsc::Sender<String>,
        reply: oneshot::Sender<Result<(), SSRDenoError>>,
    },
}

/// Owns the channel to a dedicated dev worker thread that runs a single
/// Deno `MainWorker` with `DevModuleLoader` (source-level module loading).
pub struct DevIsolateHandle {
    tx: tokio::sync::mpsc::Sender<DevWorkerMsg>,
    render_timeout_ms: u64,
}

impl DevIsolateHandle {
    pub fn spawn(
        max_heap_size_mb: usize,
        render_timeout_ms: u64,
        project_root: PathBuf,
    ) -> Result<Self, SSRDenoError> {
        let (tx, rx) = tokio::sync::mpsc::channel::<DevWorkerMsg>(1);
        let (init_tx, init_rx) = mpsc::sync_channel::<Result<(), String>>(1);

        // Per-thread index so `top -H`, `gdb info threads`, profilers can
        // distinguish workers when the user runs multiple DevModeBundles.
        static DEV_WORKER_INDEX: AtomicUsize = AtomicUsize::new(0);
        let idx = DEV_WORKER_INDEX.fetch_add(1, Ordering::Relaxed);

        std::thread::Builder::new()
            .name(format!("deno-dev-worker-{idx}"))
            .spawn(move || {
                super::dev_worker::dev_worker_thread_main(
                    rx,
                    init_tx,
                    max_heap_size_mb,
                    project_root,
                )
            })
            .map_err(|e| {
                SSRDenoError::WorkerInit(format!("Failed to spawn dev isolate thread {idx}: {e}"))
            })?;

        init_rx
            .recv()
            .map_err(|_| {
                SSRDenoError::WorkerInit(
                    "Dev isolate thread exited unexpectedly during init".into(),
                )
            })?
            .map_err(SSRDenoError::WorkerInit)?;

        Ok(Self {
            tx,
            render_timeout_ms,
        })
    }

    pub fn block_on_render(
        &self,
        bundle_id: &str,
        args_json: &str,
    ) -> Result<String, SSRDenoError> {
        let (reply_tx, reply_rx) = oneshot::channel::<Result<String, SSRDenoError>>();

        self.tx
            .blocking_send(DevWorkerMsg::Render {
                bundle_id: bundle_id.to_string(),
                args_json: args_json.to_string(),
                render_timeout_ms: self.render_timeout_ms,
                reply: reply_tx,
            })
            .map_err(|_| SSRDenoError::WorkerDied("Deno dev worker thread has exited".into()))?;

        reply_rx.blocking_recv().map_err(|_| {
            SSRDenoError::WorkerDied("Deno dev worker thread exited before reply".into())
        })?
    }

    pub fn start_render_chunked(
        &self,
        bundle_id: &str,
        args_json: &str,
    ) -> Result<ChunkedRenderResult, SSRDenoError> {
        let (reply_tx, reply_rx) = oneshot::channel::<Result<(), SSRDenoError>>();
        let (chunk_tx, chunk_rx) = tokio::sync::mpsc::channel::<String>(64);

        self.tx
            .blocking_send(DevWorkerMsg::RenderChunked {
                bundle_id: bundle_id.to_string(),
                args_json: args_json.to_string(),
                render_timeout_ms: self.render_timeout_ms,
                chunk_tx,
                reply: reply_tx,
            })
            .map_err(|_| SSRDenoError::WorkerDied("Deno dev worker thread has exited".into()))?;

        Ok((chunk_rx, reply_rx))
    }

    pub fn block_on_load_entry(
        &self,
        entry_path: &str,
        resolve_alias: HashMap<String, String>,
    ) -> Result<(), SSRDenoError> {
        let (reply_tx, reply_rx) = oneshot::channel::<Result<(), SSRDenoError>>();

        self.tx
            .blocking_send(DevWorkerMsg::LoadEntry {
                entry_path: entry_path.to_string(),
                resolve_alias,
                reply: reply_tx,
            })
            .map_err(|_| SSRDenoError::WorkerDied("Deno dev worker thread has exited".into()))?;

        reply_rx.blocking_recv().map_err(|_| {
            SSRDenoError::WorkerDied("Deno dev worker thread exited before reply".into())
        })?
    }
}
