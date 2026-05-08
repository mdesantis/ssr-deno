use std::sync::Arc;

use tokio::sync::{mpsc, oneshot};

pub use ssr_deno_core::SSRDenoError;

/// Chunk receiver and completion channel returned by chunked render.
pub(crate) type ChunkedRenderResult = (
    mpsc::Receiver<String>,
    oneshot::Receiver<Result<(), SSRDenoError>>,
);

// ---------------------------------------------------------------------------
// Wire protocol between the Ruby thread and each Deno worker thread
// ---------------------------------------------------------------------------

pub(crate) enum WorkerMsg {
    LoadBundle {
        bundle_id: String,
        bundle_path: String,
        bundle_code: Arc<str>,
        script_name: &'static str,
        reply: oneshot::Sender<Result<(), String>>,
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
        chunk_tx: mpsc::Sender<String>,
        reply: oneshot::Sender<Result<(), SSRDenoError>>,
    },
    HeapStats {
        reply: oneshot::Sender<Result<String, SSRDenoError>>,
    },
}
