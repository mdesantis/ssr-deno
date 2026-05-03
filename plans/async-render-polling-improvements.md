# Async Render Polling Improvements

## Problem

The async render promise polling loop (`call_render.rs:112-135`) had three issues:

1. **MAX_POLLS (10,000) is hard-coded** and unrelated to `render_timeout_ms`. Users who configure a longer timeout can still hit the iteration limit prematurely, or vice versa.
2. **Tight CPU spin** — no sleep or yield between poll iterations. Burns CPU for the entire poll duration.
3. **Microtasks only** — `perform_microtask_checkpoint()` drains promises but not macrotasks (setTimeout, I/O). Renders that schedule macrotasks will never settle.

A secondary poll loop in `setup_require` (`mod.rs:370`) has the same hard-coded 10,000 iteration problem, though it runs at bundle-load time rather than render time. Extracted to [`setup-require-improvements.md`](setup-require-improvements.md).

## Proposed Fix

### 1. Time-based poll loop with configurable duration

Replace `MAX_POLLS: u32 = 10_000` with a wall-clock deadline derived from `render_timeout_ms`, which is already stored on `IsolateHandle` (`mod.rs:64`). Pass it to `call_render` as a fourth parameter.

### 2. Keep outer `recv_timeout` as defense-in-depth (with buffer)

Removing `recv_timeout` entirely was incorrect. If V8 is **stuck in a sync infinite JS loop**, `perform_microtask_checkpoint()` never returns, the inner deadline check is **unreachable**, and `recv()` blocks **forever** — Ruby thread hangs.

**Fix:** Keep `recv_timeout` with a 100ms buffer above `render_timeout_ms`. The inner deadline fires first for async timeouts, while the outer timeout remains as safety net for V8-stuck scenarios:

```rust
// 100ms buffer for message-passing overhead + V8-stuck safety net
let hang_timeout = Duration::from_millis(self.render_timeout_ms + 100);
match reply_rx.recv_timeout(hang_timeout) {
    Ok(result) => result,
    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(DenoError::Render(
        format!("Render process hung after {}ms", hang_timeout.as_millis())
    )),
    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(DenoError::WorkerDied(
        "Deno worker thread exited before sending a reply".into(),
    )),
}
```

### 3. Add sleep between polls

Insert `std::thread::sleep(Duration::from_micros(100))` between each microtask checkpoint.

```rust
// Before
const MAX_POLLS: u32 = 10_000;
for poll in 0..MAX_POLLS {
    isolate.perform_microtask_checkpoint();
    // check promise state
}

// After
let deadline = Instant::now() + Duration::from_millis(render_timeout_ms);
loop {
    isolate.perform_microtask_checkpoint();
    // check promise state, break if settled
    if Instant::now() >= deadline {
        return Err(DenoError::Render(format!(
            "Async render promise did not settle within {render_timeout_ms}ms timeout"
        )));
    }
    std::thread::sleep(Duration::from_micros(100));
}
```

### 4. Handle macrotasks (optional, phase 2)

If a render function uses `setTimeout` or similar, the microtask-only loop won't work. This would require:
- Running the Deno event loop (not just microtask checkpoint)
- Or detecting a "stuck" promise and falling back to a longer poll strategy

This is complex and may not be needed — most SSR render functions use `await fetch()` which resolves via microtasks, not macrotasks.

## Implementation Steps

**Note:** `setup_require` poll loop improvements were extracted to [`setup-require-improvements.md`](setup-require-improvements.md).

### [x] Step 1: Pass render_timeout_ms to call_render

**Files:** `ext/ssr_deno/src/deno_runtime_wrapper/mod.rs`, `call_render.rs`

- Add `render_timeout_ms: u64` field to `WorkerMsg::Render` enum variant (mod.rs:41-46):
  ```rust
  Render {
      bundle_id: String,
      args_json: String,
      render_timeout_ms: u64,
      reply: std::sync::mpsc::SyncSender<Result<String, DenoError>>,
  },
  ```
- Pass `self.render_timeout_ms` when sending `WorkerMsg::Render` in `block_on_render` (mod.rs:107-112):
  ```rust
  self.tx
      .blocking_send(WorkerMsg::Render {
          bundle_id: bundle_id.to_string(),
          args_json: args_json.to_string(),
          render_timeout_ms: self.render_timeout_ms,
          reply: reply_tx,
      })
  ```
- Update the worker thread's message handler to pass `render_timeout_ms` to `call_render` (mod.rs:330-338):
  ```rust
  WorkerMsg::Render {
      bundle_id,
      args_json,
      render_timeout_ms,
      reply,
  } => {
      let result = call_render(&mut worker, &bundle_id, &args_json, render_timeout_ms);
      let _ = reply.send(result);
  }
  ```
- Add `render_timeout_ms: u64` parameter to `call_render` function signature (call_render.rs:16-21)

### [x] Step 2: Replace MAX_POLLS with deadline-based loop + sleep

**File:** `ext/ssr_deno/src/deno_runtime_wrapper/call_render.rs`

- Add import at top of file:
  ```rust
  use std::time::{Duration, Instant};
  ```
  (`std::thread::sleep` called inline, no separate import needed)
- Remove `const MAX_POLLS: u32 = 10_000`
- Compute deadline: `Instant::now() + Duration::from_millis(render_timeout_ms)`
- Replace `for poll in 0..MAX_POLLS` with `while Instant::now() < deadline` + break on settled
- Add `std::thread::sleep(Duration::from_micros(100))` inside the `Pending` arm of the state match
- Error message: `"Async render promise did not settle within {render_timeout_ms}ms timeout"`
- Update stale error message at end of function (call_render.rs:170) — replaced with `unreachable!("timeout checked before Phase 2")` since early exit guarantees pending never reaches Phase 2

### [x] Step 3: Add 100ms buffer to outer recv_timeout

**File:** `ext/ssr_deno/src/deno_runtime_wrapper/mod.rs`

- Rename existing `timeout` variable to `hang_timeout` for clarity
- In `block_on_render` (`mod.rs:102-125`), keep `recv_timeout` but add 100ms buffer:
  ```rust
  let hang_timeout = Duration::from_millis(self.render_timeout_ms + 100);
  ```
- Update timeout error message to indicate "hung" (distinguish from inner deadline):
  ```rust
  Err(DenoError::Render(format!(
      "Render process hung after {}ms",
      hang_timeout.as_millis()
  )))
  ```
- Keep `RecvTimeoutError::Disconnected` arm for worker crash detection

### [x] Step 5: Add async integration test (via `test_deno_async_render.rb`)

**File:** `test/ssr/test_deno_async_render.rb` (pre-existing, fixture-based)

The existing test file already covered the async render scenarios described below. Changes made:
- Added `SSR::Deno.render_timeout_ms = 100` in `setup` (class method) with rescue for already-initialized pool
- This makes the hang test complete in ~100ms instead of ~500ms (default)

Existing coverage maps to plan requirements:

- **Sync render test:** `function render(args) { return JSON.stringify({ name: "sync" }); } globalThis.render = render;` — verify async path doesn't break sync renders
- **Async render test (microtask):** `async function render(args) { await Promise.resolve(); return JSON.stringify({ name: "async" }); } globalThis.render = render;` — verify poll loop resolves. **Do NOT use `setTimeout`** — it's a macrotask and will never settle with microtask-only polling.
- **Async render test (nested microtask):** `async function render(args) { await new Promise(r => Promise.resolve().then(r)); return JSON.stringify({ name: "nested" }); } globalThis.render = render;` — verify nested microtask chains resolve
- **Timeout test:** `async function render() { await new Promise(() => {}); return ""; } globalThis.render = render;` with short timeout (100ms) — assert `RenderError` is raised
- **Timeout boundary validation:** Already covered by Rust unit tests in `ssr_deno_core/src/lib.rs` (accepts 100/300000, rejects 99/300001). No need to duplicate in Ruby integration tests.
- **Poll loop verification:** The poll loop execution is verified implicitly by the async test passing; there is no direct way to count polls from Ruby. If the async test returns correctly, the poll loop worked.
- **Cleanup:** Ensure temp files are removed even on test failure (use `ensure` block)
- **Note:** `SSR::Deno.render_timeout_ms = 100` must be set at the top of the test file, before any `Bundle.new` call. Only one timeout value per test process — pool cannot be reset.

### [x] Step 6: Run full pipeline

```bash
bundle exec rake
```

Must pass: Rust compile, cargo:test, sample builds, Ruby tests (100% coverage), RuboCop, RBS.

### [x] Step 7: Update CHANGELOG.md

Add entry under Unreleased → Changed:

```markdown
- Async render polling: replace fixed 10,000 iteration count with configurable timeout-based deadline. Add 100µs sleep between polls to reduce CPU usage. Outer recv_timeout now has 100ms buffer to serve as V8-stuck safety net while inner deadline handles normal async timeouts.
```

## Files Changed

| File | Change |
|------|--------|
| `ext/ssr_deno/src/deno_runtime_wrapper/call_render.rs` | Replace MAX_POLLS with deadline loop + sleep, add render_timeout_ms param, update stale error message at end |
| `ext/ssr_deno/src/deno_runtime_wrapper/mod.rs` | Pass render_timeout_ms to call_render, add 100ms buffer to recv_timeout |
| `test/ssr/test_deno_async_render.rb` | Updated: added `render_timeout_ms = 100` at top, fixture-based async tests (sync, async microtask, nested microtask, timeout) |
| `CHANGELOG.md` | Add entry under Unreleased → Changed |

## Files NOT Changed

| File | Reason |
|------|--------|
| `lib/ssr/deno.rb` | No API changes |
| `sig/ssr/deno.rbs` | No signature changes |
| `ext/ssr_deno/src/lib.rs` | No core type changes |
| `ext/ssr_deno/crates/ssr_deno_core/src/lib.rs` | No validation changes (render_timeout_ms range already 100-300000) |

## Tradeoffs

- **Sleep duration (100µs)** — Tunable. Too high = slower resolution for fast promises. Too low = still burns CPU. 100µs is a reasonable default.
- **Dual timeout with buffer** — Inner deadline fires first for normal async timeouts. Outer `recv_timeout` + 100ms buffer fires only if V8 is stuck in a sync loop. This preserves defense-in-depth without double-counting the timeout.
- **Macrotasks not supported** — Documented as a known limitation. Most SSR bundles don't use setTimeout in their render path. Async tests must use `Promise.resolve()` or similar microtask-based deferral.
- **No Ruby-level async** — The Ruby thread remains blocked during render. This is by design — the pool architecture is synchronous from Ruby's perspective. Making it truly async would require a major redesign (Fiber scheduler, non-blocking FFI).

## Known Issues (Out of Scope)

### `setup_require` silent failure (mod.rs:370-372)
Extracted to [`setup-require-improvements.md`](setup-require-improvements.md).

### `was_pending` false case
If render is synchronous and returns a resolved promise, the polling loop is skipped entirely. This is correct behavior but the integration test should cover this path to ensure sync renders still work (covered by Step 5 sync render test).

## Post-Implementation Audit (Completed)

✅ **1. Stale docs audit** — `docs/ARCHITECTURE.md`, `README.md`, `plans/*.md` searched: no references to old poll-loop behavior (MAX_POLLS, iteration counts, event-loop polling) remain in non-source files.

✅ **2. Sample directories** — No stale path references in non-vendor, non-generated parts (no new samples added, no paths changed).

✅ **3. `.vscode/settings.json`** — No samples added/removed, no update needed.

✅ **4. Plan status** — Steps 1–3, 5–7 marked `[x]`; Steps 4, 8 extracted to `setup-require-improvements.md`
