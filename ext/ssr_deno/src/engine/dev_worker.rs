use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Arc, Mutex};

use deno_runtime::deno_core::url::Url;
use tokio::runtime;
use tokio::task::LocalSet;

use ssr_deno_dev_mode::{build_dev_mode_worker, DevModeMtimeCache, SharedAliasMap, SharedCjsPaths};

use super::dev_handle::DevModeWorkerMsg;
use super::render;
use super::render_chunked;

pub fn dev_worker_thread_main(
    mut rx: tokio::sync::mpsc::Receiver<DevModeWorkerMsg>,
    init_tx: mpsc::SyncSender<Result<(), String>>,
    max_heap_size_mb: usize,
    project_root: PathBuf,
    mtime_cache: Arc<DevModeMtimeCache>,
) {
    let rt = match runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            let _ = init_tx.send(Err(format!("Failed to build Tokio runtime: {e}")));
            return;
        }
    };

    LocalSet::new().block_on(&rt, async move {
        let main_module_url = match Url::parse("https://ssr-deno.local/") {
            Ok(url) => url,
            Err(e) => {
                let _ = init_tx.send(Err(format!("Cannot build worker URL: {e}")));
                return;
            }
        };

        let oom_triggered = Arc::new(AtomicBool::new(false));
        let alias_map: SharedAliasMap = Arc::new(Mutex::new(Vec::new()));
        let cjs_paths: SharedCjsPaths = Arc::new(Mutex::new(Vec::new()));

        let mut worker = match build_dev_mode_worker(
            &main_module_url,
            max_heap_size_mb,
            alias_map.clone(),
            &project_root,
            oom_triggered.clone(),
            mtime_cache,
            cjs_paths.clone(),
        ) {
            Ok(w) => w,
            Err(e) => {
                let _ = init_tx.send(Err(e));
                return;
            }
        };

        if let Err(e) = super::worker::setup_require(&mut worker) {
            let _ = init_tx.send(Err(format!("Failed to set up require: {e}")));
            return;
        }

        let _ = init_tx.send(Ok(()));

        while let Some(msg) = rx.recv().await {
            match msg {
                DevModeWorkerMsg::LoadEntry {
                    entry_path,
                    resolve_alias,
                    reply,
                } => {
                    let result = super::dev_load::dev_load_entry(
                        &mut worker,
                        &entry_path,
                        &alias_map,
                        resolve_alias,
                        &cjs_paths,
                    )
                    .await;
                    let _ = reply.send(result);
                }
                DevModeWorkerMsg::Render {
                    bundle_id,
                    args_json,
                    render_timeout_ms,
                    reply,
                } => {
                    let result = render::render(
                        &mut worker,
                        &bundle_id,
                        &args_json,
                        render_timeout_ms,
                        &oom_triggered,
                    )
                    .await;
                    let _ = reply.send(result);
                }
                DevModeWorkerMsg::RenderChunked {
                    bundle_id,
                    args_json,
                    render_timeout_ms,
                    chunk_tx,
                    reply,
                } => {
                    let result = render_chunked::render_chunked(
                        &mut worker,
                        &bundle_id,
                        &args_json,
                        render_timeout_ms,
                        chunk_tx,
                        &oom_triggered,
                    )
                    .await;
                    let _ = reply.send(result);
                }
            }
        }
    });
}
