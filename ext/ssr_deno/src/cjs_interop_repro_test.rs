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
//! - Replace `crate::sys::Sys` with a local type delegating to `std::fs`.
//! - Replace `build_dev_worker` with inline `MainWorker::bootstrap_from_options`
//!   (see [`dev_builder.rs`](../deno_runtime_wrapper/dev_builder.rs)).
//! - See [`plans/dev-mode-cjs-interop-bug.md`](../../plans/dev-mode-cjs-interop-bug.md).

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::Arc;

    use deno_runtime::deno_core::url::Url;
    use deno_runtime::worker::MainWorker;

    use crate::deno_runtime_wrapper::dev_builder::build_dev_worker;
    use crate::deno_runtime_wrapper::worker::setup_require;
    use crate::dev_module_loader::DevMtimeCache;

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

        // Indirection package: mimics React/MUI/emotion shape — `index.js`
        // does `module.exports = require('./impl.js')` so its export names
        // can only be discovered by recursing the analyzer through the
        // re-export target.
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
        (dir, root)
    }

    fn build_worker(project_root: &PathBuf) -> MainWorker {
        let mut worker = build_dev_worker(
            &Url::parse("https://ssr-deno.local/").unwrap(),
            64,
            Arc::new(std::sync::Mutex::new(Vec::new())),
            project_root,
            Arc::new(AtomicBool::new(false)),
            Arc::new(DevMtimeCache::new()),
        )
        .expect("build_dev_worker");
        // Production path calls setup_require from dev_worker_thread_main
        // before any entry load — replicate here so `globalThis.require` is
        // available to the require-shim emitted for node_modules/*.{js,cjs}.
        setup_require(&mut worker).expect("setup_require");
        worker
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

    /// Asserts `lhs === rhs` in the worker's V8 context. Returns `Ok` if
    /// equal, `Err(message)` with the actual value (JSON-stringified) when
    /// the check trips. Both arguments must be valid JS expressions; the
    /// caller is responsible for quoting string literals.
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
        let mut worker = build_worker(&root);
        let url = Url::from_file_path(root.join("control.tsx")).unwrap();
        let id = worker.js_runtime.load_main_es_module(&url).await.unwrap();
        worker.evaluate_module(id).await.unwrap();
        assert!(
            probe_is_set(&mut worker, "__ctrl"),
            "control should execute"
        );
    }

    /// Confirms that deno_node's native CJS handling works (no NpmModuleLoader).
    /// The upstream bug only triggers when the explicit `NpmModuleLoader`'s
    /// `createRequire`-based wrapper is used. Re-enable the node_modules/
    /// dispatch in `DevModuleLoader::load` and switch this to assert `!probe`.
    #[tokio::test]
    async fn native_cjs_handling_works() {
        let (_dir, root) = create_fixtures();
        let mut worker = build_worker(&root);
        let url = Url::from_file_path(root.join("entry.tsx")).unwrap();
        let id = worker.js_runtime.load_main_es_module(&url).await.unwrap();
        worker.evaluate_module(id).await.unwrap();
        assert!(
            probe_is_set(&mut worker, "__probe"),
            "entry body should execute with deno_node native CJS"
        );
    }

    /// Validates the shim's default-export path end-to-end:
    /// `import foo from 'pkg'` should yield the CJS exports object (NOT
    /// `_m.default` — the shim does no `__esModule` unwrapping).
    #[tokio::test]
    async fn shim_default_import_yields_whole_exports() {
        let (_dir, root) = create_fixtures();
        let mut worker = build_worker(&root);
        let url = Url::from_file_path(root.join("entry-default.tsx")).unwrap();
        let id = worker
            .js_runtime
            .load_main_es_module(&url)
            .await
            .expect("load_main_es_module");
        worker.evaluate_module(id).await.expect("evaluate_module");

        // defaultVal === whole exports obj → __esModule key set and equals true
        js_strict_eq(&mut worker, "globalThis.__default.__esModule", "true")
            .expect("default import should be whole exports obj with __esModule:true");
        // defaultVal.default === 42 (raw CJS export)
        js_strict_eq(&mut worker, "globalThis.__default.default", "42")
            .expect("exports.default reachable via .default");
    }

    /// Validates that the shim's static CJS-export analysis pass exposes
    /// named CJS exports as ESM named exports: `import { named } from 'pkg'`
    /// should reach `exports.named`.
    #[tokio::test]
    async fn shim_named_import_works() {
        let (_dir, root) = create_fixtures();
        let mut worker = build_worker(&root);
        let url = Url::from_file_path(root.join("entry-named.tsx")).unwrap();
        let id = worker
            .js_runtime
            .load_main_es_module(&url)
            .await
            .expect("load_main_es_module");
        worker.evaluate_module(id).await.expect("evaluate_module");
        js_strict_eq(&mut worker, "globalThis.__named", "7")
            .expect("named import should reach exports.named");
    }

    /// Verifies the shim follows `module.exports = require('./impl')` style
    /// re-exports so React/MUI/emotion-shaped packages expose their named
    /// exports through the indirection.
    #[tokio::test]
    async fn shim_named_import_through_reexport_indirection() {
        let (_dir, root) = create_fixtures();
        let mut worker = build_worker(&root);
        let url = Url::from_file_path(root.join("entry-reexport.tsx")).unwrap();
        let id = worker
            .js_runtime
            .load_main_es_module(&url)
            .await
            .expect("load_main_es_module");
        worker.evaluate_module(id).await.expect("evaluate_module");
        js_strict_eq(&mut worker, "globalThis.__strict", "'strict'")
            .expect("reexported named binding should reach impl.js");
    }

    #[tokio::test]
    #[ignore = "upstream V8 re-entrancy bug"]
    async fn bug_entry_body_skipped() {
        let (_dir, root) = create_fixtures();
        let mut worker = build_worker(&root);
        let url = Url::from_file_path(root.join("entry.tsx")).unwrap();
        let id = worker.js_runtime.load_main_es_module(&url).await.unwrap();
        let eval = worker.evaluate_module(id).await;
        assert!(eval.is_ok(), "evaluate_module: {eval:?}");
        let probe = probe_is_set(&mut worker, "__probe");
        eprintln!("BUG: __probe set={probe}");
        assert!(probe, "entry body did not execute (known upstream bug)");
    }
}
