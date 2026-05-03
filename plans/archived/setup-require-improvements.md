# setup_require Poll Loop Improvements

## Problem

The `setup_require` function in `ext/ssr_deno/src/deno_runtime_wrapper/mod.rs` has two issues:

1. **Hard-coded 10,000 iteration poll loop** — Same issue that was fixed in `call_render`. Runs at bundle-load time, but still uses an arbitrary iteration limit instead of a time-based deadline.

2. **Silent failure** — After the poll loop, it returns `Ok(())` regardless of whether the `createRequire` promise actually resolved. If the import fails, `globalThis.require` stays undefined and subsequent bundle `require()` calls fail with confusing errors instead of a clear "could not set up require" message.

> **Note on failure likelihood:** `import('node:module')` is a Deno built-in polyfill, so it should never fail in a correctly built worker. The check is defense-in-depth — guarding against future Deno version changes that might remove `node:module` or bugs in node service wiring.

> **Post-commit fix:** The 1-second deadline introduced a regression where every bundle load added ~1s of unnecessary sleep. Fixed in `setup-require-early-exit-fix.md` — deadline reduced to 10ms.

---

## Design Decisions

### Deadline: 10ms deadline (not configurable)

`call_render` uses the configurable `render_timeout_ms` because render time varies by bundle complexity. `setup_require` runs the same microtask every time (`import('node:module')` + `createRequire`) — it resolves in under 1ms on a warm isolate. The initial 1s deadline was reduced to 10ms in a follow-up fix to avoid unnecessary sleep on every bundle load.

### No in-loop promise-state check

Unlike `call_render` (which checks the promise state each iteration via `v8::Global<v8::Promise>`), `setup_require` stores the promise on the JS side (`globalThis.__ssr_require_promise`) and the poll loop runs with no V8 scope chain active. Entering a scope chain inside the loop would be more expensive than the sleep itself. Instead, we poll for the full deadline with `std::thread::sleep(100µs)` between checkpoints, then verify once at the end via `worker.execute_script()`. The sleep keeps CPU usage low during the short window.

### Return type kept as `Result<(), String>`

The function is called by `load_bundle_in_worker`, which also returns `Result<(), String>`. Changing the return type to `DenoError` would ripple into that caller unnecessarily. The error string from `worker.execute_script()` (for the post-loop verification check) already carries the JS error message, and the existing `load_bundle_in_worker` → `load_bundle` → `DenoError::BundleLoad` chain wraps it correctly.

---

## Implementation Steps

### [x] Step 1: Add `Instant` import

**File:** `ext/ssr_deno/src/deno_runtime_wrapper/mod.rs`

Change line 4 from:
```rust
use std::time::Duration;
```
to:
```rust
use std::time::{Duration, Instant};
```

(`call_render.rs` uses the same grouped import style — consistency.)

### [x] Step 2: Replace hard-coded loop with deadline-based poll + sleep

**File:** `ext/ssr_deno/src/deno_runtime_wrapper/mod.rs`

Replace lines 369–372:
```rust
    let isolate = worker.js_runtime.v8_isolate();
    for _ in 0..10_000 {
        isolate.perform_microtask_checkpoint();
    }
```
with:
```rust
    let isolate = worker.js_runtime.v8_isolate();
    let deadline = Instant::now() + Duration::from_millis(10);
    while Instant::now() < deadline {
        isolate.perform_microtask_checkpoint();
        std::thread::sleep(Duration::from_micros(100));
    }
```

This matches the pattern used in `call_render` (time-based deadline + 100µs sleep), minus the per-iteration promise-state check (not possible without a scope chain — see Design Decisions).

### [x] Step 3: Add post-poll verification check

**File:** `ext/ssr_deno/src/deno_runtime_wrapper/mod.rs`

Replace line 374 (`Ok(())`) with:
```rust
    worker
        .execute_script(
            "<ssr-deno:require-verify>",
            r#"
            if (typeof globalThis.require === 'undefined') {
                throw new Error('createRequire failed - globalThis.require is undefined');
            }
            "#.to_string().into(),
        )
        .map_err(|e| format!("setup_require failed: {e}"))
```

This uses `worker.execute_script()` (the same API already used at the top of `setup_require`) to verify that `globalThis.require` is defined. If the promise rejected, `require` will be `undefined` and the script throws. The error propagates through the existing chain:

```
setup_require → "setup_require failed: createRequire failed..."
load_bundle_in_worker → "Failed to set up require: setup_require failed: ..."
load_bundle → DenoError::BundleLoad("Failed to set up require: ...")
Ruby Bundle.new → SSR::Deno::JsRuntimeInitializationError (from BundleLoad mapping in lib.rs:72)
```

### [x] Step 4: Update `docs/ARCHITECTURE.md`

**File:** `docs/ARCHITECTURE.md`, line 120

Current: "polls the microtask queue until `globalThis.require` is available via `createRequire`."

Replace with: "polls the microtask queue with a 10ms deadline until `globalThis.require` is available via `createRequire`; raises `BundleLoad` error if the import fails."

---

## Files Changed

| File | Change |
|------|--------|
| `ext/ssr_deno/src/deno_runtime_wrapper/mod.rs` | Add `Instant` import; replace `for 0..10_000` with deadline-based poll + sleep; add post-poll verification via `execute_script` |
| `docs/ARCHITECTURE.md` | Update `setup_require` description to mention deadline and error behavior |

## Files NOT Changed

| File | Reason |
|------|--------|
| `lib/ssr/deno.rb` | No API changes |
| `sig/ssr/deno.rbs` | No signature changes |
| `ext/ssr_deno/src/lib.rs` | `BundleLoad` mapping already exists (line 72); no new error variants needed |
| `ext/ssr_deno/crates/ssr_deno_core/src/lib.rs` | `DenoError::BundleLoad` already defined; no new variant needed |
| `ext/ssr_deno/src/deno_runtime_wrapper/call_render.rs` | Already fixed (see `archived/async-render-polling-improvements.md`) |
| `test/ssr/test_deno_errors.rb` | No new error type to test; existing `BundleLoad` mapping is already covered |
| `test/ssr/test_integration_node_builtins.rb` | Node builtins integration test already exercises the success path |

---

## Verification

1. Bundle load with `node_builtins: true` → `globalThis.require` is set (existing integration test covers this)
2. Poll loop uses wall-clock deadline instead of hard-coded iteration count (visual inspection)
3. If `createRequire` import fails for any reason → error at `Bundle.new` time with clear message
4. `bundle exec rake` passes (Rust compile, `cargo test -p ssr_deno_core`, sample builds, Ruby tests, RuboCop, RBS, SimpleCov 100% line + 100% branch)

### Testability note

The failure path (Step 3) is difficult to trigger from Ruby tests because `import('node:module')` always succeeds in a correctly built Deno worker. The verification check is defense-in-depth — it's validated by code review (does the `execute_script` call look correct?) rather than by a test that triggers the failure. If a future Deno version changes `node:module` behavior, the check catches it at bundle load time instead of producing opaque `require is not defined` errors inside user bundles.

---

## Optional Improvement (Not in Scope)

**Idempotency guard:** `setup_require` runs on every bundle load, but `globalThis.require` is already set after the first load in a worker. Adding an early return (`if globalThis.require is defined → skip`) would avoid re-running the async import + poll loop for subsequent bundles loaded into the same isolate. Worth doing if multiple bundles per isolate becomes common.
