use std::collections::HashMap;
use std::path::Path;

use deno_runtime::deno_core::url::Url;
use deno_runtime::worker::MainWorker;

pub fn build_dev_worker(
    _main_module: &Url,
    _max_heap_size_mb: usize,
    _resolve_aliases: HashMap<String, String>,
    _project_root: &Path,
) -> Result<MainWorker, String> {
    Err("build_dev_worker not yet implemented".into())
}
