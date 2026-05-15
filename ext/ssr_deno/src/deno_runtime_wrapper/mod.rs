use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

pub use ssr_deno_core::SSRDenoError;

pub(crate) mod heap_stats;
pub(crate) mod render;
pub(crate) mod render_chunked;
pub(crate) mod watchdog;

pub(crate) mod builder;
pub(crate) mod handle;
pub(crate) mod pool;
pub(crate) mod types;
pub(crate) mod worker;

#[cfg(feature = "dev-mode")]
pub(crate) mod dev_handle;
#[cfg(feature = "dev-mode")]
pub(crate) mod dev_load;
#[cfg(feature = "dev-mode")]
pub(crate) mod dev_worker;

pub use pool::IsolatePool;

// ---------------------------------------------------------------------------
// Script name interning — avoids unbounded `Box::leak` on bundle reloads
// ---------------------------------------------------------------------------

/// Cache of leaked script name strings. `MainWorker::execute_script` requires
/// `&'static str`, so we must leak — but we intern by value so each unique
/// filename is leaked at most once regardless of how many reloads occur.
///
/// In Vite dev mode, content-hashed filenames produce new unique names across
/// rebuilds, causing unbounded map growth over the session. This is a deliberate
/// tradeoff: each leaked string is ~100 bytes, and even a thousand rebuilds
/// costs ~100KB — negligible for a dev session. A bounded LRU cache would add
/// complexity without meaningful benefit.
static SCRIPT_NAMES: OnceLock<Mutex<HashMap<String, &'static str>>> = OnceLock::new();

/// Returns a `&'static str` for the given script name, reusing a previously
/// interned value if available. At most one leak per unique filename.
///
/// Intentionally allocates twice on miss (one for the leak, one as map key).
/// We expect a few script names total — the extra allocation is negligible
/// and avoids the complexity of `HashMap<&'static str, &'static str>`.
fn intern_script_name(name: &str) -> &'static str {
    let map = SCRIPT_NAMES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = map.lock().unwrap_or_else(|e| e.into_inner());

    if let Some(&cached) = guard.get(name) {
        return cached;
    }

    let leaked: &'static str = Box::leak(name.to_owned().into_boxed_str());
    guard.insert(name.to_owned(), leaked);
    leaked
}
