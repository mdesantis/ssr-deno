# Async Render Polling Improvements

## Problem

The async render promise polling loop (`call_render.rs:110-130`) has three issues:

1. **MAX_POLLS (10,000) is hard-coded** and unrelated to `render_timeout_ms`. Users who configure a longer timeout can still hit the iteration limit prematurely, or vice versa.
2. **Tight CPU spin** — no sleep or yield between poll iterations. Burns CPU for the entire poll duration.
3. **Microtasks only** — `perform_microtask_checkpoint()` drains promises but not macrotasks (setTimeout, I/O). Renders that schedule macrotasks will never settle.

## Proposed Fix

### 1. Time-based poll loop with configurable duration

Replace `MAX_POLLS: u32 = 10_000` with a wall-clock deadline derived from `render_timeout_ms`. Pass the timeout down from `IsolatePool` → `IsolateHandle` → `call_render`.

```rust
// Before
const MAX_POLLS: u32 = 10_000;
for poll in 0..MAX_POLLS { ... }

// After
let deadline = Instant::now() + Duration::from_millis(render_timeout_ms);
while Instant::now() < deadline {
    isolate.perform_microtask_checkpoint();
    // check promise state
    // short sleep between polls
}
```

### 2. Add sleep between polls

Insert a short sleep (e.g., `std::thread::sleep(Duration::from_micros(100))`) between each microtask checkpoint. This:
- Yields CPU to other threads / the OS scheduler
- Reduces power consumption during async waits
- Still allows fast resolution (100µs is negligible compared to typical async I/O latency)

### 3. Handle macrotasks (optional, phase 2)

If a render function uses `setTimeout` or similar, the microtask-only loop won't work. This would require:
- Running the Deno event loop (not just microtask checkpoint)
- Or detecting a "stuck" promise and falling back to a longer poll strategy

This is complex and may not be needed — most SSR render functions use `await fetch()` which resolves via microtasks, not macrotasks.

## Implementation Steps

### [ ] Step 1: Pass render_timeout_ms to call_render

**Files:** `ext/ssr_deno/src/deno_runtime_wrapper/mod.rs`, `call_render.rs`

- Add `render_timeout_ms` parameter to `call_render`
- Pass it through from `IsolateHandle::block_on_render`
- Compute `Instant::now() + Duration::from_millis(render_timeout_ms)` as deadline

### [ ] Step 2: Replace MAX_POLLS with deadline-based loop + sleep

**File:** `ext/ssr_deno/src/deno_runtime_wrapper/call_render.rs`

- Remove `const MAX_POLLS: u32 = 10_000`
- Replace `for poll in 0..MAX_POLLS` with `while Instant::now() < deadline`
- Add `std::thread::sleep(Duration::from_micros(100))` after each checkpoint
- Error message: "Async render promise did not settle within {timeout_ms}ms timeout"

### [ ] Step 3: Update Rust unit tests

**Files:** `ext/ssr_deno/crates/ssr_deno_core/src/lib.rs`

- No changes needed to `ssr_deno_core` — timeout validation already exists
- Verify existing tests still pass

### [ ] Step 4: Add async integration test

**Files:** `test/ssr/test_integration_async.rb`

- Create a test bundle with an async render function (e.g., `async function render(args) { await Promise.resolve(); return '<html>async</html>'; }`)
- Verify it produces correct HTML
- Verify timeout fires correctly when promise is slow

### [ ] Step 5: Run full pipeline

```bash
bundle exec rake
```

Must pass: Rust compile, cargo:test, sample builds, Ruby tests (100% coverage), RuboCop, RBS.

## Files Changed

| File | Change |
|------|--------|
| `ext/ssr_deno/src/deno_runtime_wrapper/call_render.rs` | Replace MAX_POLLS with deadline loop + sleep |
| `ext/ssr_deno/src/deno_runtime_wrapper/mod.rs` | Pass render_timeout_ms to call_render |
| `test/ssr/test_integration_async.rb` | New async integration test |
| `CHANGELOG.md` | Entry under Unreleased |

## Tradeoffs

- **Sleep duration (100µs)** — Tunable. Too high = slower resolution for fast promises. Too low = still burns CPU. 100µs is a reasonable default.
- **Macrotasks not supported** — Documented as a known limitation. Most SSR bundles don't use setTimeout in their render path.
- **No Ruby-level async** — The Ruby thread remains blocked during render. This is by design — the pool architecture is synchronous from Ruby's perspective. Making it truly async would require a major redesign (Fiber scheduler, non-blocking FFI).
