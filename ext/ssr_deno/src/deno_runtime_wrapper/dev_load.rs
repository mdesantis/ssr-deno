use std::collections::HashMap;

use deno_runtime::worker::MainWorker;

use super::SSRDenoError;

pub fn dev_load_entry(
    _worker: &mut MainWorker,
    _entry_path: &str,
    _resolve_alias: &HashMap<String, String>,
) -> Result<(), SSRDenoError> {
    Err(SSRDenoError::BundleLoad(
        "dev_load_entry not yet implemented".into(),
    ))
}
