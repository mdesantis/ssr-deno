# Dev-Mode CJS↔ESM Interop Bug

**Status (2026-05-14)** ◐ PARTIAL WORKAROUND. The synthetic `require()` shim sidesteps the V8 re-entrancy for shallow npm imports (react alone, react+react-dom alone, emotion alone all load fine). But mid/deep MUI dependency graphs (~30+ components via `__ssr_imports__`) still trigger the silent body-skip — `evaluate_module` returns `Ok(())` but `globalThis.render` is never assigned. See "2026-05-14 investigation" below for the bisection.

`NpmModuleLoader` integration is reverted. The upstream bug is unfixed; this document remains as the canonical write-up for the eventual issue filing + as a record of what we explored.

Embedded `deno_runtime 0.255.0`. ESM entry that imports a CJS-wrapped npm package goes through `NpmModuleLoader` → `translate_cjs_to_esm` cleanly; `load_main_es_module` returns `Ok(ModuleId)`; `evaluate_module(id).await` returns `Ok(())` — but the entry's top-level body **never executes**. No exception. No error. `globalThis` is silently unchanged.

This plan documents the bug for upstream filing + tracks our local workaround options. The integration code from step 13 (`NpmModuleLoader` + `DenoCjsCodeAnalyzer` + `NodeCodeTranslator` wiring in [`real_npm_types.rs`](../ext/ssr_deno/src/real_npm_types.rs) and the async `node_modules` branch in [`dev_module_loader.rs`](../ext/ssr_deno/src/dev_module_loader.rs)) was **structurally correct**. The block was upstream.

## Local workaround (evolved)

### C-lite shim (original)

`DevModuleLoader::load` no longer dispatches `node_modules/**/*.{js,cjs}` through `NpmModuleLoader`. Instead it returns a synthetic ESM shim:

```js
const _m = globalThis.require("/abs/path/to/file.js"); export default _m;
```

`globalThis.require = createRequire('file:///')` from `node:module` (set up by `setup_require` in `deno_runtime_wrapper/worker.rs`). The require runs at user-eval time, not during V8's `op_import_sync`, so the re-entrancy never triggers. `NpmModuleLoader` / `CjsTracker` / `DenoCjsCodeAnalyzer` / `NodeCodeTranslator` are no longer instantiated, and `real_npm_types.rs` is back to just `build_dev_npm_resolver`. Conditions stay `["node", "import"]`.

This is option C-lite from the workaround list below.

### Named-export static analysis (2026-05-14)

The shim now statically analyses CJS sources via `deno_ast::analyze_cjs` to discover export names, then emits `export const NAME = _m.NAME;` for each one. Supports recursive re-export indirection (`module.exports = require('./impl')`). This closes the original semantic gap — `import { X } from 'pkg'` now works for statically-analysable CJS exports.

### ESM detection (2026-05-14)

Packages shipping ESM `.js` files via the `import` condition (e.g. `react-transition-group/esm/index.js`, which starts with `export { default as ... }`) were erroneously wrapped in the require() shim. The shim's `analyze_cjs` found zero exports in ESM code → V8 linking error (`does not provide an export named 'X'`).

Fixed with a two-layer check:
1. `package.json` `"type": "module"` field (via `pkg_json_resolver`)
2. Content-based fallback `looks_like_esm`: reads the file, strips `use strict`, checks first token for `import`/`export`

### Subpackage resolution fallback (2026-05-14)

Packages like `dom-helpers` ship subdirectories (`addClass/`, `removeClass/`, …) each with their own `package.json` that redirects via `"module": "../esm/addClass.js"`. The `NodeResolver`'s `legacy_main_resolve` has a path-traversal guard:

```rust
if !guess.starts_with(package_path) {
    return Err(ModuleNotFoundError { ... });
}
```

`guess` = `dom-helpers/esm/addClass.js`, `package_path` = `dom-helpers/addClass/` — the `../` escapes the subdirectory, so the guard rejects the resolution even though the target is within the same root package.

Fixed with `try_resolve_subpackage`: manual walk of subpath directories + `package.json` resolution via `pkg_json_resolver`. Triggers only when `NodeResolver` fails.

### Permissions expanded

Added `allow_env: Some(vec![])` and `allow_sys: Some(vec![])` — required by npm packages that read `process.env.NODE_ENV` and call `os.platform()` / `os.arch()` during require() init.

### Current semantic gap: V8 re-entrancy on deep graphs

The shim works for **shallow** npm imports. Verified individually:
- `import { StrictMode } from 'react'` → ✓
- `import { renderToString } from 'react-dom/server'` → ✓
- `import createEmotionServer from '@emotion/server/create-instance'` → ✓
- All four imports together → ✓

But `evaluate_module` returns `Ok(())` with `globalThis.render` unset when the entry imports `__ssr_imports__`, which pulls in ~30 MUI component files (each importing from `@mui/material`, `@mui/icons-material`, etc.). The deep transitive chain involves `.mjs` re-export shims (`*@emotion/**/dist/*.cjs.mjs`) that are loaded as genuine ESM. When V8 evaluates these `.mjs` modules, their `export { ... } from './foo.cjs.js'` statement triggers loading of the CJS file, which gets our require() shim. The shim's `globalThis.require(...)` runs deno_node's CJS loader, which may call `op_import_sync` internally for `.mjs` sub-files — re-entering V8's `Module::Evaluate()` and marking the outer entry as "Evaluated" without its body ever running.

## Environment

| Crate | Version |
|---|---|
| `deno_runtime` | `=0.255.0` (features `["hmr"]`) |
| `deno_core` | `=0.400.0` |
| `deno_resolver` | `=0.78.0` (features `["deno_ast"]`) |
| `deno_ast` | `=0.53.1` (features `["transpiling"]`) |
| `node_resolver` | `=0.85.0` |
| v8 | `147.4.0` |
| Embedder | custom `MainWorker::bootstrap_from_options`, single-isolate per-bundle, GVL released for render |

Project layout: Rails 8 + Vite + MUI/emotion/React 19 SSR. Real entry references in a side-project (not public). Reduced repro entries below.

## Symptom

```
[ssr-deno:dev_load] after load_main_es_module: module_id=299 entry=file:///.../ssr-demos.tsx
[ssr-deno:dev_load] PRE-evaluate globalThis: {"render_type":"undefined","has_render_key":false,"has_probe":false,"global_count":207}
[ssr-deno:dev_load] evaluate_module result: Ok(())
[ssr-deno:dev_load] POST-evaluate globalThis: {"render_type":"undefined","has_render_key":false,"has_probe":false,"global_count":207}
```

`evaluate_module` returns `Ok(())`. The entry's `globalThis.__probe = 'top'` line (literal line 1 after `import`) doesn't fire. `globalThis.render` not set. Global property count identical pre- and post-evaluate.

For a bare entry (no imports) on the same worker:
```
PRE:  global_count=207, render_type=undefined
POST: global_count=209, render_type=function
```
Same flow, same code, different outcome. The presence of a CJS-wrapped import in the entry's graph is the trigger.

## Bisection table

| Entry contents | Body runs? |
|---|---|
| `globalThis.render = () => ...` (no imports) | ✓ |
| `import { x } from './_local.ts'` then assignment | ✓ |
| `import { createRequire } from "node:module"; const r = createRequire(...);` (no actual require call) | ✓ |
| `import { StrictMode } from 'react'` (CJS-wrapped via NodeCodeTranslator) | ✗ |
| Hand-written `const mod = createRequire(import.meta.url)("/abs/react/index.js")` at top-level | ✗ |

Final two collapse to the same trigger: **synchronous `require(<absolute-cjs-path>)` from ESM top-level**.

## Module-graph state

For the failing case (`import 'react'`), `DevModuleLoader` issues two loads:

1. `file:///.../entrypoints/ssr-demos.tsx` — sync transpile via `deno_ast` (project source)
2. `file:///.../node_modules/react/index.js` — async via `NpmModuleLoader::load`

Source returned for #2 (CJS→ESM wrapper synthesized by `NodeCodeTranslator::translate_cjs_to_esm`):

```js
import { createRequire as __internalCreateRequire, Module as __internalModule } from "node:module";
const require = __internalCreateRequire(import.meta.url);
let mod;
if (import.meta.main) {
  mod = __internalModule._load("/abs/path/react/index.js", null, true)
} else {
  mod = require("/abs/path/react/index.js");
}
export const Activity = mod["Activity"];
// … ~60 more named re-exports …
export const StrictMode = mod["StrictMode"];
// …
export default mod;
const __deno_export_1__ = mod;
export { __deno_export_1__ as "module.exports" };
```

For the wrapper, `import.meta.main === false` (it's imported, not the main module). The `else` branch executes: `mod = require(<cjs-path>)`. Synchronous CJS load via `deno_node`.

For the entry (ssr-demos.tsx), `import.meta.main === true`. Its source is the user's `.tsx` after `deno_ast` transpile — looks fine, no transformation issues.

## Where it likely originates (read of deno_core source)

[`deno_core-0.400.0/modules/map.rs:1401-1417`](https://github.com/denoland/deno_core/blob/0.400.0/core/modules/map.rs#L1401):

```rust
pub fn mod_evaluate<'s, 'i>(
    self: &Rc<Self>,
    scope: &mut v8::PinScope<'s, 'i>,
    id: ModuleId,
) -> impl Future<Output = Result<(), CoreError>> + use<> {
    v8::tc_scope!(tc_scope, scope);

    let module = self
        .get_handle(id)
        .map(|handle| v8::Local::new(tc_scope, handle))
        .expect("ModuleInfo not found");
    let mut status = module.get_status();

    // If the module is already evaluated, return early as there's nothing to do
    if status == v8::ModuleStatus::Evaluated {
        return Either::Left(futures::future::ready(Ok(())));
    }

    assert_eq!(
        status,
        v8::ModuleStatus::Instantiated,
        "Module not instantiated: {} ({})",
        self.get_name_by_id(id).unwrap(),
        id,
    );
    // … actual evaluation …
}
```

`mod_evaluate` returns `Ok(())` immediately when V8 status is already `Evaluated`. The `assert_eq!` further down proves status can only be `Evaluated` OR `Instantiated` at this point (anything else panics). Since we don't crash and our probe confirms the body never ran, the entry's status must be `Evaluated` at the time of our call — **without ever having had its body executed**.

## Hypothesised trigger path

Speculation — couldn't verify without patching `deno_core`:

1. `load_main_es_module(entry_url)` resolves transitive graph: `entry → react-wrapper → node:module`. All three reach status `Instantiated`. ✓
2. Our `evaluate_module(entry_id).await` calls V8's `Module::Evaluate()` on the entry.
3. V8 walks the graph in post-order. Evaluates `node:module` (extension code, sync). Then evaluates react-wrapper body.
4. Wrapper body runs `mod = createRequire(import.meta.url)("/abs/react/index.js")`.
5. `createRequire` is the deno_node polyfill (in `01_require.js`). The returned `require` calls into `Module._load` → resolves filename → invokes `_extensions[".js"]` → `loadMaybeCjs` or `loadESMFromCJS` based on `op_require_is_maybe_cjs(filename)`.
6. For files where the CJS-vs-ESM detection is ambiguous (or for `.mjs`/`.wasm` extensions), [`loadESMFromCJS`](https://github.com/denoland/deno/blob/main/ext/node/polyfills/01_require.js) is invoked. That function calls [`op_import_sync`](https://github.com/denoland/deno_core/blob/0.400.0/core/ops_builtin.rs#L658):
   ```js
   function loadESMFromCJS(module, filename, code) {
     const namespace = op_import_sync(
       url.pathToFileURL(filename).toString(),
       code,
     );
     // …
   }
   ```
7. `op_import_sync` (deno_core) calls `module_map_rc.mod_evaluate_sync(scope, module_id)` ([`ops_builtin.rs:689`](https://github.com/denoland/deno_core/blob/0.400.0/core/ops_builtin.rs#L689)).
8. `mod_evaluate_sync` ([`map.rs:1584`](https://github.com/denoland/deno_core/blob/0.400.0/core/modules/map.rs#L1584)) calls `module.evaluate(tc_scope)` on the loaded module's V8 handle. V8's spec-conformant `Module.Evaluate()` walks the reachable graph.
9. **Speculation**: this nested synchronous `module.evaluate()` — running inside V8 while V8 is *already* inside the outer evaluation of our entry — interacts with V8's "evaluating top level" state in a way that marks the outer entry as `Evaluated` without running its body. V8's spec algorithm uses `[[Status]]`, `[[EvaluationError]]`, `[[Index]]`, `[[CycleRoot]]` etc.; nested evaluation from a synchronous host call (op_import_sync) inside the outer evaluation may collide with the DFS ancestor tracking.

The hypothesis is consistent with all observations, but the precise V8 transition is unverified.

## What I tried (none worked)

- `load_side_es_module` instead of `load_main_es_module` — same silent-skip
- Skip `setup_require` at worker init — same
- `run_up_to_duration(500ms)` after `evaluate_module` to drain async — same
- Add an `eprintln!` immediately before/after `evaluate_module` to confirm timing — confirmed Ok returns synchronously fast
- Probe `globalThis` via `execute_script` pre- and post-evaluate — confirmed unchanged
- Disable the `dev-mode/deno_ast` feature on `deno_resolver` so the wrapper synthesis uses `NotImplementedModuleExportAnalyzer` — panics on first CJS load (expected: placeholder panics)
- Set `NodeCodeTranslatorMode::Disabled` — wrapper returns raw CJS source; V8's ESM parser rejects `exports`/`require` syntax (expected: not silent)

## What hints the issue exists

[Maintainer @bartlomieju in discussion #23468](https://github.com/denoland/deno/discussions/23468):
> "[Setting up `globalThis.require = createRequire(...)`] is somewhat of a hack and will probably not work in all situations. CJS modules with internal `require` statements may not function reliably."

This is direct acknowledgement that CJS embedder support is documented as best-effort. Our symptom is one specific failure mode of that limitation.

[Issue #28919](https://github.com/denoland/deno/issues/28919) — `npm:react-dom/server` hangs Deno CLI on exit. Different symptom (hang vs silent skip) but same package family. Was closed without a public resolution.

[Issue #27881](https://github.com/denoland/deno/issues/27881) — open, embedder npm docs gap. Tangential.

[Issue #26649](https://github.com/denoland/deno/issues/26649) — CJS analyzer should handle ESM re-exports. Tangential.

No issue in `denoland/deno` or `denoland/deno_core` matches our exact symptom (silent body-skip in embedder + CJS-wrapped import).

## Minimal Rust repro (for upstream)

Sketch for the issue filing. Self-contained — drop into a Cargo project, add a `node_modules/foo-cjs/` with a minimal `index.js`:

```rust
// Cargo.toml
// deno_runtime = { version = "=0.255.0", features = ["hmr"] }
// deno_core = "=0.400.0"
// deno_resolver = { version = "=0.78.0", features = ["deno_ast"] }
// deno_ast = { version = "=0.53.1", features = ["transpiling"] }
// node_resolver = "=0.85.0"

// 1. Construct MainWorker with:
//    - DevModuleLoader (or any ModuleLoader that routes node_modules/* via
//      NpmModuleLoader<DenoCjsCodeAnalyzer<Sys>, DenoInNpmPackageChecker,
//      DenoIsBuiltInNodeModuleChecker, ByonmNpmResolver<Sys>, Sys>)
//    - Permissions: allow_read=[project_root], deny everything else
//    - node_services with the same Byonm resolver
// 2. Entry: write to disk at <project>/entry.tsx:
//        import { foo } from 'foo-cjs'
//        globalThis.__probe = 'top'
//        globalThis.result = foo
// 3. <project>/node_modules/foo-cjs/package.json:
//        { "name": "foo-cjs", "main": "index.js" }
//    <project>/node_modules/foo-cjs/index.js:
//        module.exports.foo = 42
// 4. Run:
let module_id = worker.js_runtime
    .load_main_es_module(&Url::from_file_path(&entry_path).unwrap())
    .await
    .unwrap();
worker.evaluate_module(module_id).await.unwrap();  // returns Ok(())

// 5. Probe:
let global_state = worker.execute_script(
    "<probe>",
    r#"JSON.stringify({
        has_render: 'render' in globalThis,
        has_probe: '__probe' in globalThis,
        result_value: globalThis.result,
    })"#.to_string().into(),
).unwrap();
// Expected: {"has_probe":true,"result_value":42}
// Actual:   {"has_probe":false,"has_render":false,"result_value":null}
```

Expected: body runs, `result` is 42.
Actual: `evaluate_module` returns Ok, body never ran, `result` undefined.

## Workaround paths (local)

In rough order of "least invasive → most decoupled":

### A. Pre-bundle CJS packages (regress npm to bundling)

Keep `DevModuleLoader` for `.tsx` project source. Have Vite/Rolldown pre-bundle npm into a single `dist/server/vendor.bundle.js` (UMD or IIFE) at boot. `DevModeBundle#initialize` calls `execute_script` to load the vendor bundle before `dev_load_entry`. User's entry uses globals (eg `globalThis.React`) instead of `import 'react'`.

Pros: works today, no CJS evaluation through V8 ESM at all. Step 13 wiring becomes dead and can be removed.
Cons: regression to pre-step-13 architecture for npm. Bundling step needed (defeats partial goal of "no build step"). Hot reload only applies to project source.

### B. File upstream issue + wait

Stay on step 13 wiring. Document dev-mode as "blocked by deno_runtime CJS embedder gap." Mark MUI/emotion/React-using apps as unsupported in dev-mode until upstream fix.

Pros: zero ongoing work. Future deno_runtime release may fix it. Repro is concrete; minimal-LOC issue.
Cons: indeterminate timeline. Dev-mode v1 ships with severe practical limitation. Side-project blocked.

### C. Custom CJS loader bypassing `op_import_sync`

Replace `NpmModuleLoader` with our own CJS handler. For files in `node_modules/`:

1. Read file source from disk
2. Run a separate `execute_script` with a CJS-style wrapper:
   ```js
   const module = { exports: {} };
   const exports = module.exports;
   const require = function(spec) {
       // Recursive: synchronously call into Rust to load + execute spec
   };
   (function(module, exports, require, __filename, __dirname) {
       <CJS source>
   })(module, exports, require, "<abs path>", "<dirname>");
   globalThis.__cjs_modules["/abs/path"] = module.exports;
   ```
3. Recursive `require` implemented as Rust→JS bridge that returns synchronously
4. After all dependencies are pre-loaded into `globalThis.__cjs_modules`, return a synthetic ESM that does `export const X = globalThis.__cjs_modules['/abs/path'].X` etc.

Bypasses `op_import_sync` entirely (everything runs through `execute_script`, never `mod_evaluate*`). User's entry then evaluates normally.

Pros: independent of upstream. Custom but contained (~300-500 LOC).
Cons: re-implements significant chunk of `NodeCodeTranslator`. Edge cases (circular deps, dynamic require, exports detection) need handling. Risk of subtle bugs.

### D. Patch `deno_runtime` privately

Vendor `deno_runtime` / `deno_core` / `deno_resolver` patches via `[patch.crates-io]`. Adjust `op_import_sync` to NOT use `mod_evaluate_sync` when called reentrantly inside an outer evaluation, OR adjust the wrapper synthesized by `NodeCodeTranslator` to use `await import(...)` instead of synchronous `require(...)`.

Pros: actual upstream fix attempt — would inform the issue we file.
Cons: invasive maintenance burden. Forking deno_runtime not ideal long-term.

### E. Restructure dev mode entry: hoist all npm to deferred dynamic imports

Transform user code:
```tsx
import { StrictMode } from 'react'
globalThis.render = ...
```
→
```tsx
const deferred = (async () => {
    const { StrictMode } = await import('react')
    globalThis.render = ...
})()
globalThis.__ssr_ready = deferred
```

Wait for `__ssr_ready` before first render. Top-level await is what `evaluate_module` handles natively (the async resolution path).

Pros: minimal Rust change, mostly Ruby/transpile-side rewrite. Tests easily.
Cons: requires user code rewrite OR auto-transformation in DevModuleLoader's transpile pass. Auto-transformation is fragile (regex / AST manipulation). Async pattern leaks into user code.

## Recommendation

**Done**: C-lite workaround with named-export static analysis, ESM detection, and subpackage resolution fallback. Individual npm imports (react, react-dom, emotion) load and render correctly through the shim. The `require()` shim avoids the V8 re-entrancy for these shallow cases.

**Blocked**: deep MUI dependency graphs (~30+ components) still trigger the silent body-skip. The `.mjs` re-export shims in `@emotion/*` packages appear to re-enter V8's module evaluator through `op_import_sync`.

**Next steps to unblock** (in priority order):

1. **Verify `.mjs` as the re-entrancy vector.** Write a reduced test: entry imports `@emotion/server/create-instance` (which routes to a `.cjs.mjs` file via exports `import` condition). If that alone triggers the bug, `.mjs` is the gateway. If not, the re-entrancy requires the deeper MUI graph and the vector is likely `@mui/material`'s internal module structure.

2. **Shim `.mjs` files too.** If `.mjs` is confirmed as the gateway, extend the shim to also wrap `.mjs` files: same `const _m = globalThis.require(...); export ...` pattern. Trade-off: genuine ESM features (top-level await, `import.meta`) break for `.mjs` files — but the `.cjs.mjs` files in emotion don't use those features.

3. **Pre-load vendor bundle via `execute_script`.** Option A from the workaround list: have Vite pre-bundle npm deps into a `vendor.bundle.js` (IIFE), load it via `execute_script` before the entry. This bypasses the V8 module evaluator entirely for npm code. Only project source goes through `DevModuleLoader`. Regresses to a build step but guarantees no re-entrancy.

**Medium-term**: file the upstream issue with the Rust repro above. If a fix lands, drop the shim and reinstate `NpmModuleLoader`.

## Open questions

1. **Exact V8 mechanism**: precise transition that marks our entry `Evaluated` without running body. Likely needs reading V8's `SourceTextModule::ModuleEvaluate` and instrumenting. Out of scope for the issue filing — symptom is reproducible without the deep mechanism known.
2. **Does the bug occur in Deno CLI (`deno run`) too?** Reduced test: write a `.ts` entry that does `import { StrictMode } from 'npm:react'` + `globalThis.x = 1` + `console.log(globalThis.x)`. If logs `1`, the bug is embedder-specific. If logs `undefined`, it's a deno_core / V8 bug surface that affects CLI too. **Worth running before filing** — narrows scope significantly.
3. **Are other CJS-bridge entrypoints affected?** Our case is `require(<cjs>)` inside an ESM wrapper. What about `await import('npm:...')` dynamic import in the entry? Plausibly works (dynamic = async = different code path). Not tested.
4. **Does `node:module._load` (the `if (import.meta.main)` branch) trigger the same bug?** Probably yes since it calls into the same CJS loader. Untested.
5. **(2026-05-14) Can we prevent ESM `.mjs` files from loading natively?** The `.mjs` re-export shims (`@emotion/**/dist/*.cjs.mjs`) are the gateway that lets CJS evaluation re-enter V8's module evaluator. If we shimmed `.mjs` too (wrapping them in `require()` like `.js`/`.cjs`), the re-entrancy path would be blocked. Trade-off: `.mjs` files that genuinely need ESM features (top-level await, import.meta) would break.
6. **(2026-05-14) Are `@mui` packages valid for dev-mode at all?** MUI v6+ ships `.mjs` entry points that re-export from `.js` CJS bundles. Even if `.mjs` were shimmed, the internal `require()` chain within `@mui/material` is ~300 modules deep — likely exceeds any reasonable time budget for dev mode. Worth profiling once the re-entrancy is resolved.

## 2026-05-14 investigation

**Setup**: side-project (`~/Sviluppo/denpro`), Rails 8 + MUI/emotion/React 19, ~30 components. Entry at `app/frontend/entrypoints/ssr-app.tsx`.

**Bisection** (all tested standalone, same worker, `@` → `app/frontend` alias):

| Entry content | `globalThis.render` set? |
|---|---|
| `globalThis.render = ...` (no imports) | ✓ |
| `import { StrictMode } from 'react'` only | ✓ |
| `import { renderToString } from 'react-dom/server'` only | ✓ |
| `import createEmotionServer from '@emotion/server/create-instance'` only | ✓ |
| All four npm imports together | ✓ |
| `import * as __c0 from '@/components/app/dashboard.tsx'` only | ✓ |
| `import * from __ssr_imports__` (30+ components) | ✗ |

**Trigger chain hypothesis**: `__ssr_imports__` → `@/components/app/dashboard.tsx` → `@mui/material/...` → emotion `.cjs.mjs` re-export shims → `export { ... } from './foo.cjs.js'` → V8 evaluates shim for `.cjs.js` → `globalThis.require(...)` → deno_node CJS loader → (some path) `op_import_sync` → `Module::Evaluate()` nested inside the outer evaluation → outer entry marked "Evaluated" without body execution.

**New code shipped** (2026-05-14):
- `src/dev_module_loader.rs`: `looks_like_esm()`, `is_esm_inside_node_modules()`, `try_resolve_subpackage()`, `analyze_cjs_exports()` + named export shim generation
- `src/require_loader.rs`: `DevNodeRequireLoader` (reads files from disk for `require()`)
- `src/deno_runtime_wrapper/dev_builder.rs`: `allow_env`, `allow_sys` permissions
- `src/deno_runtime_wrapper/worker.rs`: `setup_require` visibility `pub(super)` → `pub(crate)`
- `test/ssr/test_deno_bundle.rb`: reset `@_bundles_created` in setup (pre-existing test-order fix)
- `src/cjs_interop_repro_test.rs`: tests for default import, named import, re-export indirection

## Action items if we proceed

- [ ] Build standalone Rust repro (separate Cargo project, not embedded in this gem)
- [ ] Test against Deno CLI to determine if embedder-specific
- [ ] File upstream issue at `denoland/deno` with `embedder` label, link to repro
- [ ] Decide: revert step 13 vs leave wired with docs
- [ ] Update `plans/ssr-source-dev-mode.md` §step 13 status: blocked → revert or limited

## Cross-references

- [Main plan](ssr-source-dev-mode.md) step 13 — implementation that hit this wall
- [Follow-ups](dev-mode-followups.md) — non-blocking cleanups for dev mode
- [Discussion #23468](https://github.com/denoland/deno/discussions/23468) — maintainer-acknowledged CJS embedder unreliability
- [Issue #28919](https://github.com/denoland/deno/issues/28919) — adjacent `react-dom/server` hang in CLI
