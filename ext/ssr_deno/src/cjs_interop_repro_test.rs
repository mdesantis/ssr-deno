//! Reproducer for the CJS→ESM interop bug.
//!
//! `evaluate_module` returns `Ok(())` but the entry's top-level body never
//! executes when the import graph contains a CJS-wrapped npm package.
//! The root cause is a V8 re-entrancy issue with `op_import_sync` called
//! from the CJS wrapper's synchronous `require()` during outer evaluation.
//!
//! See `plans/dev-mode-cjs-interop-bug.md` for full analysis.
//!
//! ## Expected vs actual
//!
//! - Expected: entry body runs → `__probe` is set on `globalThis`
//! - Actual: `evaluate_module` returns `Ok`, but `__probe` is `undefined`
//!
//! Run with:
//!   bash -c "set -a; source .env; set +a; cd ext/ssr_deno; cargo test -p ssr_deno --lib -- cjs_interop --nocapture"
//!
//! Or via Rake (compiles V8 first):
//!   bundle exec rake cargo:test:ssr_deno
//!
//! ## Note on test runtime vs prod
//!
//! Prod runs the worker inside `LocalSet::new().block_on(&rt, ...)` because
//! Deno web extensions (eg React 19's scheduler via `MessageChannel`) call
//! `deno_unsync::spawn_local` which requires a LocalSet on the current
//! thread. The minimal repro entry here (`import { foo } from 'foo-cjs'`)
//! doesn't exercise those code paths, so `#[tokio::test]`'s default
//! multi-thread runtime is sufficient. If this test is ever extended to
//! load `react-dom/server` or similar, switch to a `current_thread`
//! runtime wrapped in `LocalSet`.

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::{Arc, atomic::AtomicBool};

    use deno_runtime::deno_core::url::Url;
    use deno_runtime::worker::MainWorker;
    use std::sync::atomic::{AtomicU64, Ordering};

    use crate::dev_module_loader::{DevMtimeCache, SharedAliasMap};
    use crate::deno_runtime_wrapper::dev_builder::build_dev_worker;

    // ── helpers ──────────────────────────────────────────────────────────

    static DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

    /// Temporary directory that auto-cleans on drop. Unique name per creation.
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

        // CJS package: node_modules/foo-cjs/
        let pkg_dir = root.join("node_modules").join("foo-cjs");
        std::fs::create_dir_all(&pkg_dir).expect("mkdir pkg");
        std::fs::write(
            pkg_dir.join("package.json"),
            r#"{"name":"foo-cjs","main":"index.js"}"#,
        )
        .expect("write package.json");
        std::fs::write(
            pkg_dir.join("index.js"),
            "Object.defineProperty(exports, '__esModule', { value: true }); exports.default = 42; exports.foo = 42;\n",
        )
        .expect("write index.js");

        // Entry file: imports from the CJS package, sets a probe
        std::fs::write(
            root.join("entry.tsx"),
            "import { foo } from 'foo-cjs';\nglobalThis.__probe = 'top';\nglobalThis.__result = foo;\n",
        )
        .expect("write entry.tsx");

        // Expected-entry file: same source but no npm import — proves body
        // runs when the graph contains only project-local modules.
        std::fs::write(
            root.join("control.tsx"),
            "globalThis.__probe_control = 'ok';\nglobalThis.__result_control = 42;\n",
        )
        .expect("write control.tsx");

        (dir, root)
    }

    // ── tests ────────────────────────────────────────────────────────────

    /// Control case: entry WITHOUT npm imports. Body MUST execute.
    #[tokio::test]
    async fn control_entry_body_executes() {
        let (_dir, root) = create_fixtures();
        let mut worker = build_worker(&root);

        let entry_url = Url::from_file_path(root.join("control.tsx")).unwrap();
        let module_id = worker.js_runtime.load_main_es_module(&entry_url).await.unwrap();
        worker.evaluate_module(module_id).await.unwrap();

        let probe = probe_is_set(&mut worker, "__probe_control");
        eprintln!("CONTROL: __probe_control set={probe}");
        assert!(probe, "control entry body should execute");
    }

    /// Bug watchdog: PASSES today while the upstream bug is present. Fails
    /// the day the bug is fixed — that's the signal to drop `#[ignore]` from
    /// `cjs_import_skips_entry_body`, remove this watchdog, and close the
    /// upstream issue.
    ///
    /// Not gated by `#[ignore]` so CI actively monitors for the fix.
    #[tokio::test]
    async fn cjs_import_bug_watchdog_still_broken() {
        let (_dir, root) = create_fixtures();
        let mut worker = build_worker(&root);

        let entry_url = Url::from_file_path(root.join("entry.tsx")).unwrap();
        let module_id = worker
            .js_runtime
            .load_main_es_module(&entry_url)
            .await
            .expect("load_main_es_module");
        worker
            .evaluate_module(module_id)
            .await
            .expect("evaluate_module Ok (silent-skip is the bug)");

        let probe = probe_is_set(&mut worker, "__probe");
        eprintln!("WATCHDOG: __probe set={probe} (false = bug still present)");

        // This test PASSES while the bug exists. When it starts failing, the
        // upstream fix has landed → flip both tests:
        //   1. Remove #[ignore] from cjs_import_skips_entry_body
        //   2. Delete this watchdog
        //   3. Update plans/dev-mode-cjs-interop-bug.md status
        assert!(
            !probe,
            "Upstream bug appears fixed: entry body executed when CJS-wrapped \
             import is in graph. Drop #[ignore] from cjs_import_skips_entry_body, \
             remove this watchdog, update plans/dev-mode-cjs-interop-bug.md."
        );
    }

    /// Bug case: entry importing from a CJS-wrapped npm package.
    /// Body silently does NOT execute despite evaluate_module returning Ok.
    /// Ignored by default — remove `#[ignore]` when upstream fix lands.
    #[tokio::test]
    #[ignore = "upstream V8 re-entrancy bug (see plans/dev-mode-cjs-interop-bug.md)"]
    async fn cjs_import_skips_entry_body() {
        let (_dir, root) = create_fixtures();
        let mut worker = build_worker(&root);

        let entry_url = Url::from_file_path(root.join("entry.tsx")).unwrap();

        // Load the module graph
        let module_id = worker
            .js_runtime
            .load_main_es_module(&entry_url)
            .await
            .expect("load_main_es_module");

        // Evaluate
        let eval_result = worker.evaluate_module(module_id).await;
        assert!(
            eval_result.is_ok(),
            "evaluate_module returned Err: {:?}. The known bug is that it \
             returns Ok(()) silently without running the entry body. An Err \
             here means upstream behavior changed — re-investigate.",
            eval_result.as_ref().err()
        );

        let probe = probe_is_set(&mut worker, "__probe");

        eprintln!("CJS-ENTRY: __probe set={probe}");

        // The entry body SHOULD execute. Currently fails due to upstream V8
        // re-entrancy bug (see plans/dev-mode-cjs-interop-bug.md).
        assert!(
            probe,
            "Entry body did not execute. Known upstream bug — see \
             plans/dev-mode-cjs-interop-bug.md. \
             When this passes without #[ignore], the upstream fix has landed."
        );
    }

    // ── helpers ──────────────────────────────────────────────────────────

    fn build_worker(project_root: &PathBuf) -> MainWorker {
        let main_module_url = Url::parse("https://ssr-deno.local/").unwrap();
        let resolve_aliases: SharedAliasMap = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mtime_cache = Arc::new(DevMtimeCache::new());
        let oom_triggered = Arc::new(AtomicBool::new(false));

        build_dev_worker(
            &main_module_url,
            64,
            resolve_aliases,
            project_root,
            oom_triggered,
            mtime_cache,
        )
        .expect("build_dev_worker")
    }

    /// Check if a globalThis property is set. Uses throw-on-undefined to
    /// avoid extracting `v8::Global<v8::Value>` as a Rust string.
    fn probe_is_set(worker: &mut MainWorker, name: &str) -> bool {
        let script = format!(
            "if (typeof globalThis.{name} === 'undefined') throw new Error('_UNSET_');"
        );
        worker
            .execute_script("<probe>", script.to_string().into())
            .is_ok()
    }
}
