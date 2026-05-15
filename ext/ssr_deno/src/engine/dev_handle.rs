use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};

use tokio::sync::oneshot;

use super::types::ChunkedRenderResult;
use super::SSRDenoError;
use ssr_deno_dev_mode::DevModeMtimeCache;

pub(crate) enum DevModeWorkerMsg {
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
///
/// `render_timeout_ms` is **not** cached on the handle — it is read by Ruby
/// from `SSR::Deno::Config` on every render and passed through the FFI, so
/// Rails apps that set `config.ssr_deno.render_timeout_ms` after the handle
/// is created still get the new value applied.
pub struct DevModeIsolateHandle {
    tx: tokio::sync::mpsc::Sender<DevModeWorkerMsg>,
    cache: Arc<DevModeMtimeCache>,
}

impl DevModeIsolateHandle {
    pub fn spawn(max_heap_size_mb: usize, project_root: PathBuf) -> Result<Self, SSRDenoError> {
        let cache = Arc::new(DevModeMtimeCache::new());
        let cache_for_worker = cache.clone();
        let (tx, rx) = tokio::sync::mpsc::channel::<DevModeWorkerMsg>(1);
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
                    cache_for_worker,
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

        Ok(Self { tx, cache })
    }

    pub fn block_on_render(
        &self,
        bundle_id: &str,
        args_json: &str,
        render_timeout_ms: u64,
    ) -> Result<String, SSRDenoError> {
        let (reply_tx, reply_rx) = oneshot::channel::<Result<String, SSRDenoError>>();

        self.tx
            .blocking_send(DevModeWorkerMsg::Render {
                bundle_id: bundle_id.to_string(),
                args_json: args_json.to_string(),
                render_timeout_ms,
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
        render_timeout_ms: u64,
    ) -> Result<ChunkedRenderResult, SSRDenoError> {
        let (reply_tx, reply_rx) = oneshot::channel::<Result<(), SSRDenoError>>();
        let (chunk_tx, chunk_rx) = tokio::sync::mpsc::channel::<String>(64);

        self.tx
            .blocking_send(DevModeWorkerMsg::RenderChunked {
                bundle_id: bundle_id.to_string(),
                args_json: args_json.to_string(),
                render_timeout_ms,
                chunk_tx,
                reply: reply_tx,
            })
            .map_err(|_| SSRDenoError::WorkerDied("Deno dev worker thread has exited".into()))?;

        Ok((chunk_rx, reply_rx))
    }

    /// Check if any loaded module's source file has changed on disk since
    /// the last `dev_load_entry`. Pure filesystem stat on the caller thread —
    /// no worker message needed (the mtime cache is `Arc`-shared).
    pub fn check_stale(&self) -> bool {
        self.cache.any_stale()
    }

    pub fn block_on_load_entry(
        &self,
        entry_path: &str,
        resolve_alias: HashMap<String, String>,
    ) -> Result<(), SSRDenoError> {
        let (reply_tx, reply_rx) = oneshot::channel::<Result<(), SSRDenoError>>();

        self.tx
            .blocking_send(DevModeWorkerMsg::LoadEntry {
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
