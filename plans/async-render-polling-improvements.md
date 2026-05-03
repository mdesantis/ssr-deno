# Async Render Polling Improvements

## Problem

The async render promise polling loop (`call_render.rs:110-130`) has three issues:

1. **MAX_POLLS (10,000) is hard-coded** and unrelated to `render_timeout_ms`. Users who configure a longer timeout can still hit the iteration limit prematurely, or vice versa.
2. **Tight CPU spin** — no sleep or yield between poll iterations. Burns CPU for the entire poll duration.
3. **Microtasks only** — `perform_microtask_checkpoint()` drains promises but not macrotasks (setTimeout, I/O). Renders that schedule macrotasks will never settle.

A secondary poll loop in `setup_require` (`mod.rs:367`) has the same hard-coded 10,000 iteration problem, though it runs at bundle-load time rather than render time.

## Proposed Fix

### 1. Time-based poll loop with configurable duration

Replace `MAX_POLLS: u32 = 10_000` with a wall-clock deadline derived from `render_timeout_ms`, which is already stored on `IsolateHandle` (`mod.rs:63`). Pass it to `call_render` as a fourth parameter.

### 2. Remove outer `recv_timeout` — rely on inner deadline exclusively

Currently there are two nested timeouts:
- Outer: `recv_timeout(render_timeout_ms)` in `block_on_render` (`mod.rs:103-122`)
- Inner: `MAX_POLLS` loop in `call_render`

With the deadline-based inner loop, the outer `recv_timeout` becomes redundant. Because message-passing overhead eats into the budget, the outer timeout would always fire first, making the inner deadline's error path unreachable and the worker thread's result silently dropped.

**Fix:** Replace `recv_timeout(timeout)` with `recv()` (no timeout). The deadline inside `call_render` is now the sole timeout — when it fires, the error flows back through the channel naturally.

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

### [ ] Step 1: Pass render_timeout_ms to call_render

**Files:** `ext/ssr_deno/src/deno_runtime_wrapper/mod.rs`, `call_render.rs`

- Add `render_timeout_ms: u64` parameter to `call_render`
- Pass `self.render_timeout_ms` from `IsolateHandle` at the call site (mod.rs:333)
- `block_on_render` already holds the value in `self.render_timeout_ms` — no new plumbing needed above `IsolateHandle`

### [ ] Step 2: Replace MAX_POLLS with deadline-based loop + sleep

**File:** `ext/ssr_deno/src/deno_runtime_wrapper/call_render.rs`

- Remove `const MAX_POLLS: u32 = 10_000`
- Compute deadline: `Instant::now() + Duration::from_millis(render_timeout_ms)`
- Replace `for poll in 0..MAX_POLLS` with `loop` + deadline check
- Add `std::thread::sleep(Duration::from_micros(100))` at end of each iteration
- Error message: `"Async render promise did not settle within {timeout_ms}ms timeout"`

### [ ] Step 3: Remove outer recv_timeout in block_on_render

**File:** `ext/ssr_deno/src/deno_runtime_wrapper/mod.rs`

- In `block_on_render` (`mod.rs:101-123`), replace `recv_timeout(timeout)` with `recv()`
- Remove the `RecvTimeoutError::Timeout` match arm (no longer reachable)
- Keep the `RecvTimeoutError::Disconnected` arm for worker crash detection

### [ ] Step 4: Update setup_require poll loop (optional improvement)

**File:** `ext/ssr_deno/src/deno_runtime_wrapper/mod.rs`

- The `setup_require` function (`mod.rs:367`) has its own `for _ in 0..10_000` loop for `createRequire` bootstrap
- This runs at bundle-load time, not render time — lower priority
- Apply the same deadline+sleep pattern for consistency, or defer to a follow-up

### [ ] Step 5: Add async integration test

**File:** `test/ssr/test_integration_async.rb` (new)

- **Sync render test:** `function render(args) { return JSON.stringify({ name: "sync" }); } globalThis.render = render;` — verify async path doesn't break sync renders
- **Async render test:** `async function render(args) { await new Promise(r => setTimeout(r, 0)); return JSON.stringify({ name: "async" }); } globalThis.render = render;` — verify poll loop resolves
- **Timeout test:** `async function render() { await new Promise(() => {}); return ""; } globalThis.render = render;` with short timeout — assert `RenderError` is raised
- **Different timeout values:** Test with 100ms, 1000ms, 5000ms to verify behavior scales correctly
- **Cleanup:** Ensure temp files are removed even on test failure (use `ensure` block)

### [ ] Step 6: Run full pipeline

```bash
bundle exec rake
```

Must pass: Rust compile, cargo:test, sample builds, Ruby tests (100% coverage), RuboCop, RBS.

### [ ] Step 7: Update CHANGELOG.md

Add entry under Unreleased → Changed:

```markdown
- Async render polling: replace fixed 10,000 iteration count with configurable timeout-based deadline. Add 100µs sleep between polls to reduce CPU usage. Removed redundant outer `recv_timeout` — single timeout now applies to entire render operation.
```

### [ ] Step 8: (Optional) Fix `setup_require` error handling

If time allows, add a promise state check in `setup_require` (mod.rs:367-371) similar to `call_render`'s final check:

```rust
// After the poll loop, check if the require import actually resolved
for _ in 0..100 {
    isolate.perform_microtask_checkpoint();
}
// Check if globalThis.require is defined
```

This is out of scope for the main PR but worth documenting.

## Files Changed

| File | Change |
|------|--------|
| `ext/ssr_deno/src/deno_runtime_wrapper/call_render.rs` | Replace MAX_POLLS with deadline loop + sleep, add render_timeout_ms param, update stale error message at end |
| `ext/ssr_deno/src/deno_runtime_wrapper/mod.rs` | Pass render_timeout_ms to call_render, remove outer recv_timeout, update setup_require loop |
| `test/ssr/test_integration_async.rb` | New async integration test (sync, async, timeout, different values) |
| `CHANGELOG.md` | Add entry under Unreleased → Changed |

## Tradeoffs

- **Sleep duration (100µs)** — Tunable. Too high = slower resolution for fast promises. Too low = still burns CPU. 100µs is a reasonable default.
- **Single timeout** — Removing the outer `recv_timeout` means the worker thread is the sole timeout authority. If the worker crashes without sending a reply, `recv()` returns `Disconnected` which maps to `WorkerDied`.
- **Macrotasks not supported** — Documented as a known limitation. Most SSR bundles don't use setTimeout in their render path.
- **No Ruby-level async** — The Ruby thread remains blocked during render. This is by design — the pool architecture is synchronous from Ruby's perspective. Making it truly async would require a major redesign (Fiber scheduler, non-blocking FFI).
- **Zero timeout (0ms)** — If user sets `render_timeout_ms = 0`, deadline = `Instant::now()`. First `Instant::now() >= deadline` check is a race condition (same instant). Behavior: one microtask checkpoint then timeout. Add validation to reject 0 or treat as "no limit".

## Known Issues (Out of Scope)

### `setup_require` silent failure (mod.rs:367-371)
The `setup_require` function doesn't check if the `createRequire` promise actually resolved. After the loop, it returns `Ok(())` regardless. If the import fails, `globalThis.require` is undefined and bundles fail later with confusing errors. Should add a promise state check similar to `call_render`'s final check.

### Stale error message at end of `call_render` (call_render.rs:164-168)
After the polling loop exits, the `v8::PromiseState::Pending` arm at the end of `call_render` has the old "maximum number of event-loop polls" message. Should be updated to match the new timeout message format.

### `was_pending` false case
If render is synchronous and returns a resolved promise, the polling loop is skipped entirely. This is correct behavior but the integration test should cover this path to ensure sync renders still work.
