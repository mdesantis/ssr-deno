use std::collections::HashMap;
use std::path::Path;

use deno_runtime::deno_core::url::Url;
use deno_runtime::worker::MainWorker;

use crate::dev_module_loader::{set_aliases, SharedAliasMap};

use super::SSRDenoError;

/// Evaluates a `.tsx` / `.ts` / `.js` entry module and registers its
/// `globalThis.render` under `globalThis.__ssr_bundles[entry_path]`.
///
/// **Single-shot per worker lifetime.** V8 caches modules in its module map
/// keyed by URL; calling this function a second time on the same worker
/// returns the cached `ModuleId` without re-fetching from `DevModuleLoader`,
/// even if source files have changed on disk. The post-eval namespace script
/// also clears `globalThis.render = undefined`, so a second call fails with
/// "Entry did not assign a function to globalThis.render".
///
/// The auto-reload path (step 11) must drop+respawn the worker and call this
/// fresh — never call twice on the same worker.
///
/// **On load failure**, the worker is left with a half-evaluated module
/// graph and partially-populated transpile cache. The caller should treat
/// the worker as poisoned and respawn it.
///
/// **Path canonicalization.** `entry_path` may be relative; the function
/// canonicalizes it to an absolute URL for module loading but uses the raw
/// `entry_path` argument as the `__ssr_bundles[]` key. The Ruby `DevBundle`
/// must pass the *same* string at both load time and render time, otherwise
/// the render-time lookup misses.
pub async fn dev_load_entry(
    worker: &mut MainWorker,
    entry_path: &str,
    alias_map: &SharedAliasMap,
    new_aliases: &HashMap<String, String>,
) -> Result<(), SSRDenoError> {
    // 1. Update the shared alias map (DevModuleLoader reads it lazily).
    set_aliases(alias_map, new_aliases);

    // 2. Resolve entry to absolute URL.
    let abs_path = Path::new(entry_path).canonicalize().map_err(|e| {
        SSRDenoError::BundleLoad(format!("Cannot resolve entry path '{entry_path}': {e}"))
    })?;
    let entry_url = Url::from_file_path(&abs_path).map_err(|_| {
        SSRDenoError::BundleLoad(format!(
            "Cannot convert entry path to URL: {}",
            abs_path.display()
        ))
    })?;

    // 3. Load the entry module and all its dependencies through DevModuleLoader.
    //    This triggers transpilation of .ts/.tsx and source-map registration.
    let module_id = worker
        .js_runtime
        .load_main_es_module(&entry_url)
        .await
        .map_err(|e| SSRDenoError::BundleLoad(format!("Failed to load entry module: {e}")))?;

    // 4. Evaluate the module (executes top-level code, which should assign
    //    `globalThis.render`).
    worker
        .evaluate_module(module_id)
        .await
        .map_err(|e| SSRDenoError::BundleLoad(format!("Failed to evaluate entry module: {e}")))?;

    // 5. Register `globalThis.render` under `__ssr_bundles[entry_path]`.
    let entry_path_js =
        serde_json::to_string(entry_path).expect("serde_json::to_string cannot fail for &str");

    let namespace_script = format!(
        r#"(function(id) {{
            if (typeof globalThis.__ssr_bundles === 'undefined') {{
                globalThis.__ssr_bundles = {{}};
            }}
            if (globalThis.__ssr_bundles[id]) {{
                throw new Error('Bundle ' + JSON.stringify(id) +
                    ' already loaded; respawn the worker to reload it.');
            }}
            if (typeof globalThis.render !== 'function') {{
                throw new Error('Entry did not assign a function to globalThis.render');
            }}
            globalThis.__ssr_bundles[id] = {{ render: globalThis.render }};
            globalThis.render = undefined;
        }})({entry_path_js});"#
    );

    worker
        .execute_script("<ssr-deno:namespace>", namespace_script.into())
        .map(|_| ())
        .map_err(|e| {
            SSRDenoError::BundleLoad(format!("Failed to register bundle '{entry_path}': {e}"))
        })
}
