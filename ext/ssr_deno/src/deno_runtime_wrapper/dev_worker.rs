use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Arc, Mutex};

use deno_runtime::deno_core::url::Url;
use tokio::runtime;
use tokio::task::LocalSet;

use super::dev_builder::build_dev_worker;
use super::dev_handle::DevWorkerMsg;
use super::render;
use super::render_chunked;

pub fn dev_worker_thread_main(
    mut rx: tokio::sync::mpsc::Receiver<DevWorkerMsg>,
    init_tx: mpsc::SyncSender<Result<(), String>>,
    max_heap_size_mb: usize,
    project_root: PathBuf,
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
        let alias_map: crate::dev_module_loader::SharedAliasMap =
            Arc::new(Mutex::new(Vec::new()));

        let mut worker = match build_dev_worker(
            &main_module_url,
            max_heap_size_mb,
            alias_map.clone(),
            &project_root,
            oom_triggered.clone(),
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
                DevWorkerMsg::LoadEntry {
                    entry_path,
                    resolve_alias,
                    reply,
                } => {
                    let result = super::dev_load::dev_load_entry(
                        &mut worker,
                        &entry_path,
                        &alias_map,
                        &resolve_alias,
                    )
                    .await;
                    let _ = reply.send(result);
                }
                DevWorkerMsg::Render {
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
                DevWorkerMsg::RenderChunked {
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
