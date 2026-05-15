//! Reproducer for the CJS→ESM interop bug.
//!
//! `evaluate_module` returns `Ok(())` but the entry's top-level body never
//! executes when the import graph contains a CJS-wrapped npm package.
//! Root cause: V8 re-entrancy via `op_import_sync` during synchronous
//! `require()` inside the CJS→ESM wrapper.
//!
//! ## Porting to standalone Cargo project
//!
//! - Collect `[dependencies]` from [`Cargo.toml`](../Cargo.toml) starting
//!   with `deno_`, `node_`, `sys_traits`, plus `tokio`, `url`.
//! - Replace `ssr_deno_sys::Sys` with a local type delegating to `std::fs`.
//! - Replace `build_dev_mode_worker` with inline `MainWorker::bootstrap_from_options`
//!   (see [`dev_mode_builder.rs`](../../crates/ssr_deno_dev_mode/src/dev_mode_builder.rs)).
//! - See [`plans/archived/dev-mode-cjs-interop-bug.md`](../../plans/archived/dev-mode-cjs-interop-bug.md).

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::Arc;

    use deno_runtime::deno_core::url::Url;
    use deno_runtime::worker::MainWorker;

    use crate::engine::dev_load::warm_cjs_cache;
    use crate::engine::worker::setup_require;
    use ssr_deno_dev_mode::{build_dev_mode_worker, DevModeMtimeCache, SharedCjsPaths};

    static DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new() -> std::io::Result<Self> {
            let seq = DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
            let mut path = std::env::temp_dir();
            path.push(format!("cjs_repro_{pid}_{seq}", pid = std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path)?;
            Ok(Self { path })
        }
        fn path(&self) -> &PathBuf {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn create_fixtures() -> (TempDir, PathBuf) {
        let dir = TempDir::new().expect("temp dir");
        let root = dir.path().clone();
        let pkg = root.join("node_modules").join("foo-cjs");
        std::fs::create_dir_all(&pkg).expect("mkdir pkg");
        std::fs::write(
            pkg.join("package.json"),
            r#"{"name":"foo-cjs","main":"index.js"}"#,
        )
        .expect("write package.json");
        std::fs::write(
            pkg.join("index.js"),
            "Object.defineProperty(exports, '__esModule', { value: true });\n\
             exports.default = 42;\n\
             exports.named = 7;\n",
        )
        .expect("write index.js");

        let bar = root.join("node_modules").join("bar-cjs");
        std::fs::create_dir_all(&bar).expect("mkdir bar pkg");
        std::fs::write(
            bar.join("package.json"),
            r#"{"name":"bar-cjs","main":"index.js"}"#,
        )
        .expect("write bar package.json");
        std::fs::write(
            bar.join("index.js"),
            "'use strict';\n\
             if (process.env.NODE_ENV === 'production') {\n\
                 module.exports = require('./impl.js');\n\
             } else {\n\
                 module.exports = require('./impl.js');\n\
             }\n",
        )
        .expect("write bar index.js");
        std::fs::write(
            bar.join("impl.js"),
            "Object.defineProperty(exports, '__esModule', { value: true });\n\
             exports.StrictMode = 'strict';\n\
             exports.useState = function () { return 99; };\n",
        )
        .expect("write bar impl.js");
        std::fs::write(
            root.join("entry.tsx"),
            "import { default as val } from 'foo-cjs';\nglobalThis.__probe = 'top';\n",
        )
        .expect("write entry.tsx");
        std::fs::write(
            root.join("entry-default.tsx"),
            "import defaultVal from 'foo-cjs';\n\
             globalThis.__default = defaultVal;\n",
        )
        .expect("write entry-default.tsx");
        std::fs::write(
            root.join("entry-named.tsx"),
            "import { named } from 'foo-cjs';\n\
             globalThis.__named = named;\n",
        )
        .expect("write entry-named.tsx");
        std::fs::write(
            root.join("entry-reexport.tsx"),
            "import { StrictMode } from 'bar-cjs';\n\
             globalThis.__strict = StrictMode;\n",
        )
        .expect("write entry-reexport.tsx");
        std::fs::write(root.join("control.tsx"), "globalThis.__ctrl = 'ok';\n")
            .expect("write control.tsx");

        // ESM-as-.js package
        let esm_js = root.join("node_modules").join("esm-as-js");
        std::fs::create_dir_all(&esm_js).expect("mkdir esm-as-js");
        std::fs::write(
            esm_js.join("package.json"),
            r#"{"name":"esm-as-js","main":"index.js"}"#,
        )
        .expect("write esm-as-js package.json");
        std::fs::write(
            esm_js.join("index.js"),
            "/** @license MIT */\n\
             // generated by tsc\n\
             export const greeting = 'hello';\n",
        )
        .expect("write esm-as-js index.js");
        std::fs::write(
            root.join("entry-esm-as-js.tsx"),
            "import { greeting } from 'esm-as-js';\n\
             globalThis.__greeting = greeting;\n",
        )
        .expect("write entry-esm-as-js.tsx");

        // Subpackage layout
        let subpkg = root.join("node_modules").join("sub-pkg");
        std::fs::create_dir_all(&subpkg).expect("mkdir sub-pkg");
        std::fs::write(
            subpkg.join("package.json"),
            r#"{"name":"sub-pkg","main":"index.js"}"#,
        )
        .expect("write sub-pkg package.json");
        std::fs::write(
            subpkg.join("index.js"),
            "module.exports = { fallback: true };\n",
        )
        .expect("write sub-pkg index.js");
        std::fs::create_dir_all(subpkg.join("esm")).expect("mkdir sub-pkg/esm");
        std::fs::write(
            subpkg.join("esm").join("addClass.js"),
            "export default function addClass(c) { return 'esm-' + c; }\n",
        )
        .expect("write sub-pkg/esm/addClass.js");
        let addclass_dir = subpkg.join("addClass");
        std::fs::create_dir_all(&addclass_dir).expect("mkdir sub-pkg/addClass");
        std::fs::write(
            addclass_dir.join("package.json"),
            r#"{"name":"sub-pkg/addClass","module":"../esm/addClass.js"}"#,
        )
        .expect("write sub-pkg/addClass/package.json");
        std::fs::write(
            root.join("entry-subpkg.tsx"),
            "import addClass from 'sub-pkg/addClass';\n\
             globalThis.__addclass = addClass('btn');\n",
        )
        .expect("write entry-subpkg.tsx");

        (dir, root)
    }

    fn build_worker(project_root: &PathBuf) -> (MainWorker, SharedCjsPaths) {
        let cjs_paths: SharedCjsPaths = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut worker = build_dev_mode_worker(
            &Url::parse("https://ssr-deno.local/").unwrap(),
            64,
            Arc::new(std::sync::Mutex::new(Vec::new())),
            project_root,
            Arc::new(AtomicBool::new(false)),
            Arc::new(DevModeMtimeCache::new()),
            cjs_paths.clone(),
        )
        .expect("build_dev_mode_worker");
        setup_require(&mut worker).expect("setup_require");
        (worker, cjs_paths)
    }

    fn probe_is_set(worker: &mut MainWorker, name: &str) -> bool {
        worker
            .execute_script(
                "<probe>",
                format!(
                    "if (typeof globalThis.{name} === 'undefined') throw new Error('_UNSET_');"
                )
                .to_string()
                .into(),
            )
            .is_ok()
    }

    fn js_strict_eq(worker: &mut MainWorker, lhs: &str, rhs: &str) -> Result<(), String> {
        let script = format!(
            "if (!({lhs} === {rhs})) {{ \
                 throw new Error('expected ' + JSON.stringify({rhs}) + \
                                 ', got ' + JSON.stringify({lhs})); \
             }}"
        );
        worker
            .execute_script("<eq>", script.into())
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    #[tokio::test]
    async fn control() {
        let (_dir, root) = create_fixtures();
        let (mut worker, cjs_paths) = build_worker(&root);
        let url = Url::from_file_path(root.join("control.tsx")).unwrap();
        let id = worker.js_runtime.load_main_es_module(&url).await.unwrap();
        warm_cjs_cache(&mut worker, &cjs_paths).expect("warm_cjs_cache");
        worker.evaluate_module(id).await.unwrap();
        assert!(
            probe_is_set(&mut worker, "__ctrl"),
            "control should execute"
        );
    }

    #[tokio::test]
    async fn native_cjs_handling_works() {
        let (_dir, root) = create_fixtures();
        let (mut worker, cjs_paths) = build_worker(&root);
        let url = Url::from_file_path(root.join("entry.tsx")).unwrap();
        let id = worker.js_runtime.load_main_es_module(&url).await.unwrap();
        warm_cjs_cache(&mut worker, &cjs_paths).expect("warm_cjs_cache");
        worker.evaluate_module(id).await.unwrap();
        assert!(
            probe_is_set(&mut worker, "__probe"),
            "entry body should execute with deno_node native CJS"
        );
    }

    #[tokio::test]
    async fn shim_default_import_yields_whole_exports() {
        let (_dir, root) = create_fixtures();
        let (mut worker, cjs_paths) = build_worker(&root);
        let url = Url::from_file_path(root.join("entry-default.tsx")).unwrap();
        let id = worker
            .js_runtime
            .load_main_es_module(&url)
            .await
            .expect("load_main_es_module");
        warm_cjs_cache(&mut worker, &cjs_paths).expect("warm_cjs_cache");
        worker.evaluate_module(id).await.expect("evaluate_module");
        js_strict_eq(&mut worker, "globalThis.__default.__esModule", "true")
            .expect("default import should be whole exports obj with __esModule:true");
        js_strict_eq(&mut worker, "globalThis.__default.default", "42")
            .expect("exports.default reachable via .default");
    }

    #[tokio::test]
    async fn shim_named_import_works() {
        let (_dir, root) = create_fixtures();
        let (mut worker, cjs_paths) = build_worker(&root);
        let url = Url::from_file_path(root.join("entry-named.tsx")).unwrap();
        let id = worker
            .js_runtime
            .load_main_es_module(&url)
            .await
            .expect("load_main_es_module");
        warm_cjs_cache(&mut worker, &cjs_paths).expect("warm_cjs_cache");
        worker.evaluate_module(id).await.expect("evaluate_module");
        js_strict_eq(&mut worker, "globalThis.__named", "7")
            .expect("named import should reach exports.named");
    }

    #[tokio::test]
    async fn shim_named_import_through_reexport_indirection() {
        let (_dir, root) = create_fixtures();
        let (mut worker, cjs_paths) = build_worker(&root);
        let url = Url::from_file_path(root.join("entry-reexport.tsx")).unwrap();
        let id = worker
            .js_runtime
            .load_main_es_module(&url)
            .await
            .expect("load_main_es_module");
        warm_cjs_cache(&mut worker, &cjs_paths).expect("warm_cjs_cache");
        worker.evaluate_module(id).await.expect("evaluate_module");
        js_strict_eq(&mut worker, "globalThis.__strict", "'strict'")
            .expect("reexported named binding should reach impl.js");
    }

    #[tokio::test]
    async fn esm_as_js_package_loads_natively() {
        let (_dir, root) = create_fixtures();
        let (mut worker, cjs_paths) = build_worker(&root);
        let url = Url::from_file_path(root.join("entry-esm-as-js.tsx")).unwrap();
        let id = worker
            .js_runtime
            .load_main_es_module(&url)
            .await
            .expect("load_main_es_module");
        warm_cjs_cache(&mut worker, &cjs_paths).expect("warm_cjs_cache");
        worker.evaluate_module(id).await.expect("evaluate_module");
        js_strict_eq(&mut worker, "globalThis.__greeting", "'hello'")
            .expect("ESM named import should reach exported const");
    }

    #[tokio::test]
    async fn subpackage_with_parent_path_resolves_via_fallback() {
        let (_dir, root) = create_fixtures();
        let (mut worker, cjs_paths) = build_worker(&root);
        let url = Url::from_file_path(root.join("entry-subpkg.tsx")).unwrap();
        let id = worker
            .js_runtime
            .load_main_es_module(&url)
            .await
            .expect("load_main_es_module");
        warm_cjs_cache(&mut worker, &cjs_paths).expect("warm_cjs_cache");
        worker.evaluate_module(id).await.expect("evaluate_module");
        js_strict_eq(&mut worker, "globalThis.__addclass", "'esm-btn'")
            .expect("subpackage module should resolve via fallback");
    }

    #[tokio::test]
    #[ignore = "upstream V8 re-entrancy bug"]
    async fn bug_entry_body_skipped() {
        let (_dir, root) = create_fixtures();
        let (mut worker, _cjs_paths) = build_worker(&root);
        let url = Url::from_file_path(root.join("entry.tsx")).unwrap();
        let id = worker.js_runtime.load_main_es_module(&url).await.unwrap();
        let eval = worker.evaluate_module(id).await;
        assert!(eval.is_ok(), "evaluate_module: {eval:?}");
        let probe = probe_is_set(&mut worker, "__probe");
        eprintln!("BUG: __probe set={probe}");
        assert!(probe, "entry body did not execute (known upstream bug)");
    }
}
