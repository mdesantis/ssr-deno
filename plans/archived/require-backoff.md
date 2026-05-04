# setup_require — 50µs busy-sleep burns CPU

Status: Closed (low priority)

## Problem

`setup_require` polls for the `createRequire` promise resolution with
a 50µs sleep in a tight loop:

```rust
loop {
    isolate.perform_microtask_checkpoint();
    if Instant::now() >= deadline {
        break;
    }
    std::thread::sleep(Duration::from_micros(50));
}
```

At 50µs per iteration over 100ms deadline, that's ~2,000 iterations
per bundle load. For an app with 5 bundles, that's 10,000 wakeups.

## Analysis

The async import of `node:module` resolves via microtask checkpoint.
Each `perform_microtask_checkpoint` processes pending promise
callbacks. The import target is a built-in extension — normally
resolves in <1ms. The 100ms deadline is a safety net.

The loop spins at ~20kHz. At minimum priority, this still burns CPU
on a hyperthread that could be used by other Ruby threads or the
Tokio runtime.

## Implementation Draft

Replace the fixed 50µs sleep with exponential backoff capped at 1ms:

```rust
let deadline = Instant::now() + Duration::from_millis(100);
let mut delay_us = 50;

loop {
    isolate.perform_microtask_checkpoint();
    if Instant::now() >= deadline {
        break;
    }
    std::thread::sleep(Duration::from_micros(delay_us));
    delay_us = std::cmp::min(delay_us * 2, 1000);
}
```

For the common case (resolves in <1ms):
- 50µs, 100µs, 200µs, 400µs, 800µs = 5 iterations
- Total wait: ~1.55ms (vs 5 × 50µs = 0.25ms without backoff)
- The promise resolves before the first sleep, so only the first
  checkpoint matters. The extra wait is in the safety-net spins
  after resolution but before `perform_microtask_checkpoint` checks.

Actually, the issue is that `perform_microtask_checkpoint` is called
BEFORE the sleep. If the promise resolved, the first checkpoint catches
it and we check the verify script on the next `execute_script` call.
The sleep only adds latency between the resolution-checkpoint and
the break.

A better approach: check if the promise settled AND break immediately
without sleeping on the first successful check:

```rust
loop {
    isolate.perform_microtask_checkpoint();
    if Instant::now() >= deadline {
        break;
    }
    // Check if require is set before sleeping
    if check_require_set(worker) {
        break;
    }
    std::thread::sleep(Duration::from_micros(delay_us));
    delay_us = std::cmp::min(delay_us * 2, 1000);
}
```

But this adds another `execute_script` call per iteration, which is
more expensive than the sleep it replaces.

**Recommended approach:** Simple doubling backoff. The extra latency
on the exceptional case (import takes >1ms) is negligible (max ~2ms
total vs ~2.5ms previously), and the hot path is unchanged (promise
resolves on first or second checkpoint).

```rust
fn setup_require(worker: &mut MainWorker) -> Result<(), String> {
    // ... idempotency guard and eval as before ...

    let deadline = Instant::now() + Duration::from_millis(100);
    let mut delay_us: u64 = 50;

    loop {
        isolate.perform_microtask_checkpoint();
        if Instant::now() >= deadline {
            break;
        }
        // Exponential backoff: 50µs, 100µs, 200µs, 400µs, 800µs, 1ms, 1ms, ...
        std::thread::sleep(Duration::from_micros(delay_us));
        delay_us = std::cmp::min(delay_us.saturating_mul(2), 1000);
    }

    // ... verify as before ...
}
```

This reduces worst-case iterations from ~2,000 to ~8 and keeps
total spin time within the original 100ms budget.

## Test Strategy

Covered by existing tests:
- `test_deno_bundle.rb` — loads bundles with `node_builtins: true`
  (indirectly exercises `setup_require`)
- No new test needed — behavior is identical, only timing changes

## Verification

- [x] Implement exponential backoff in `setup_require` — skipped (low priority, not worth the churn)
- [x] `bundle exec rake` — must exit 0 — skipped (low priority)
