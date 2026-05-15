use std::collections::HashMap;
use std::path::Path;

use deno_runtime::deno_core::url::Url;
use deno_runtime::worker::MainWorker;

use ssr_deno_dev_mode::{drain_cjs_paths, set_aliases, SharedAliasMap, SharedCjsPaths};

use super::SSRDenoError;

/// Runs `globalThis.__cjs_cache[p] = globalThis.require(p)` for every CJS
/// path the shim has wrapped so far. Call between `load_main_es_module` and
/// `evaluate_module`: the `execute_script` boundary keeps `require()` calls
/// outside V8's module-evaluation post-order walk, so the upstream
/// re-entrancy bug (see `plans/archived/dev-mode-cjs-interop-bug.md`) cannot fire.
pub fn warm_cjs_cache(
    worker: &mut MainWorker,
    cjs_paths: &SharedCjsPaths,
) -> Result<(), SSRDenoError> {
    let paths = drain_cjs_paths(cjs_paths);
    if paths.is_empty() {
        return Ok(());
    }
    let paths_array_json = serde_json::to_string(
        &paths
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
    )
    .map_err(|e| SSRDenoError::BundleLoad(format!("Failed to encode CJS paths: {e}")))?;
    let warmup_script = format!(
        r#"(function (paths) {{
            globalThis.__cjs_cache = globalThis.__cjs_cache || Object.create(null);
            for (const p of paths) {{
                if (globalThis.__cjs_cache[p] !== undefined) continue;
                try {{
                    globalThis.__cjs_cache[p] = globalThis.require(p);
                }} catch (e) {{
                    throw new Error('CJS warmup failed for ' + p + ': ' + (e && e.stack || e));
                }}
            }}
        }})({paths_array_json});"#
    );
    worker
        .execute_script("<ssr-deno:cjs-warmup>", warmup_script.into())
        .map(|_| ())
        .map_err(|e| SSRDenoError::BundleLoad(format!("CJS warmup script failed: {e}")))
}

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
/// `entry_path` argument as the `__ssr_bundles[]` key. The Ruby `DevModeBundle`
/// must pass the *same* string at both load time and render time, otherwise
/// the render-time lookup misses.
pub async fn dev_load_entry(
    worker: &mut MainWorker,
    entry_path: &str,
    alias_map: &SharedAliasMap,
    new_aliases: &HashMap<String, String>,
    cjs_paths: &SharedCjsPaths,
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
    //    The loader pushes every `node_modules/*.{js,cjs}` path that gets
    //    wrapped in a require()-shim onto `cjs_paths`.
    let module_id = worker
        .js_runtime
        .load_main_es_module(&entry_url)
        .await
        .map_err(|e| SSRDenoError::BundleLoad(format!("Failed to load entry module: {e}")))?;

    // 4. Pre-populate `globalThis.__cjs_cache` for every shim-wrapped CJS
    //    file before `evaluate_module` runs.
    warm_cjs_cache(worker, cjs_paths)?;

    // 5. Evaluate the module (executes top-level code, which should assign
    //    `globalThis.render`).
    worker
        .evaluate_module(module_id)
        .await
        .map_err(|e| SSRDenoError::BundleLoad(format!("Failed to evaluate entry module: {e}")))?;

    // 6. Register `globalThis.render` under `__ssr_bundles[entry_path]`.
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
                const progress = typeof globalThis.__entry_progress !== 'undefined'
                    ? ' Last reached: ' + JSON.stringify(globalThis.__entry_progress) + '.'
                    : ' (no `globalThis.__entry_progress` probe found — add it to bisect.)';
                throw new Error(
                    'Entry did not assign a function to globalThis.render. ' +
                    'This is the upstream V8 silent body-skip bug ' +
                    '(see plans/archived/dev-mode-cjs-interop-bug.md).' + progress
                );
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
