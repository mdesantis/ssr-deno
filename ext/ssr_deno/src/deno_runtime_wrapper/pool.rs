use std::fs;
use std::path::Path;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;

use tokio::sync::oneshot;

use crate::deno_runtime_wrapper::intern_script_name;
use ssr_deno_core::next_index;
use ssr_deno_core::validate_pool_size;

use super::handle::IsolateHandle;
use super::types::{ChunkedRenderResult, WorkerMsg};
use super::SSRDenoError;

// ---------------------------------------------------------------------------
// IsolatePool — dispatcher of render requests across N isolates
// ---------------------------------------------------------------------------

/// A load-balancing dispatcher that owns multiple `IsolateHandle`s and
/// distributes render requests across them in round-robin fashion.
pub struct IsolatePool {
    handles: Vec<IsolateHandle>,
    counter: AtomicUsize, // Round-robin counter
}

impl IsolatePool {
    /// Creates a pool of `size` isolates, each with `max_heap_size_mb`
    /// as its V8 heap limit and `render_timeout_ms` as the render timeout.
    /// Returns an error if `size` is 0 or if any
    /// isolate thread fails to spawn.
    pub fn new(
        size: usize,
        max_heap_size_mb: usize,
        render_timeout_ms: u64,
        node_builtins: bool,
    ) -> Result<Self, SSRDenoError> {
        validate_pool_size(size)?;

        let mut handles = Vec::with_capacity(size);
        for i in 0..size {
            let handle =
                IsolateHandle::spawn(i, max_heap_size_mb, render_timeout_ms, node_builtins)?;
            handles.push(handle);
        }

        Ok(Self {
            handles,
            counter: AtomicUsize::new(0),
        })
    }

    /// Returns the number of live isolates in the pool.
    /// Currently unused externally — will be needed by heap_stats_all
    /// for per-isolate metrics reporting (see plans/archived/v8-heap-metrics.md).
    #[allow(dead_code)]
    pub fn size(&self) -> usize {
        self.handles.len()
    }

    /// Picks the next isolate in round-robin order.
    fn next_handle(&self) -> &IsolateHandle {
        let idx = next_index(&self.counter, self.handles.len());
        &self.handles[idx]
    }

    /// Dispatches a render request to the next available isolate.
    /// Blocks until the result arrives.
    pub fn dispatch_render(
        &self,
        bundle_id: &str,
        args_json: &str,
    ) -> Result<String, SSRDenoError> {
        self.next_handle().block_on_render(bundle_id, args_json)
    }

    /// Dispatches a chunked render to the next available isolate.
    /// Returns the chunk receiver and completion channel — the caller iterates
    /// chunks until the receiver returns `None`, then checks the completion
    /// channel for errors.
    pub fn dispatch_render_chunked(
        &self,
        bundle_id: &str,
        args_json: &str,
    ) -> Result<ChunkedRenderResult, SSRDenoError> {
        self.next_handle()
            .start_render_chunked(bundle_id, args_json)
    }

    /// Queries V8 heap statistics from the next available isolate.
    pub fn heap_stats(&self) -> Result<String, SSRDenoError> {
        self.next_handle().block_on_heap_stats()
    }

    /// Loads a bundle into **every** isolate by broadcasting the bundle code.
    /// Path resolution (canonicalize, symlink check) is done once — all
    /// isolates receive the same code and script name.
    pub fn load_bundle(&self, bundle_id: &str, bundle_path: &str) -> Result<(), SSRDenoError> {
        let bundle_name = Path::new(bundle_path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("(unknown)");
        let canonical = fs::canonicalize(bundle_path).map_err(|e| {
            SSRDenoError::BundleLoad(format!("Cannot resolve bundle path '{bundle_name}': {e}"))
        })?;

        // Reject symlink escapes: the resolved path must stay within the
        // directory that was originally specified.
        let original_parent = Path::new(bundle_path)
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let canonical_parent = fs::canonicalize(original_parent).map_err(|e| {
            SSRDenoError::BundleLoad(format!("Cannot resolve bundle directory: {e}"))
        })?;
        if !canonical.starts_with(&canonical_parent) {
            return Err(SSRDenoError::BundleLoad(format!(
                "Bundle file '{bundle_name}' escapes its directory via symlink"
            )));
        }

        let bundle_code = fs::read_to_string(bundle_path).map_err(|e| {
            SSRDenoError::BundleLoad(format!("Cannot read bundle file '{bundle_name}': {e}"))
        })?;

        let bundle_code: Arc<str> = bundle_code.into();

        // `MainWorker::execute_script` requires `&'static str` for the script
        // name. Interned so each unique filename is leaked at most once,
        // regardless of how many reloads occur in development.
        let script_name: &'static str = canonical
            .file_name()
            .and_then(|s| s.to_str())
            .map(intern_script_name)
            .unwrap_or("main.js");

        // Broadcast to all isolates in parallel: send all messages first, then
        // collect replies. Each worker processes independently, cutting load
        // time from O(n × eval_time) to O(eval_time).
        let mut reply_rxs = Vec::with_capacity(self.handles.len());

        for handle in &self.handles {
            let (reply_tx, reply_rx) = oneshot::channel();

            // TODO: replace dead isolate — if blocking_send fails, the worker
            // is dead but prior workers already got the bundle. No replacement
            // mechanism exists; the pool runs with fewer isolates.
            handle.blocking_send(WorkerMsg::LoadBundle {
                bundle_id: bundle_id.to_string(),
                bundle_path: bundle_path.to_string(),
                bundle_code: Arc::clone(&bundle_code),
                script_name,
                reply: reply_tx,
            })?;

            reply_rxs.push(reply_rx);
        }

        for reply_rx in reply_rxs {
            reply_rx
                .blocking_recv()
                .map_err(|_| SSRDenoError::WorkerDied("Isolate worker exited before reply".into()))?
                .map_err(SSRDenoError::BundleLoad)?;
        }

        Ok(())
    }
}
