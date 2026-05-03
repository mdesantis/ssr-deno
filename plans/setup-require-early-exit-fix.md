# setup_require Early-Exit Fix

## Problem

Commit `358cf5c` added a deadline-based poll loop to `setup_require`, but the loop always runs the **full 1-second deadline** — even when the `createRequire` promise resolves in under 1ms on a warm isolate. There is no early-exit condition.

The `call_render` poll loop (`call_render.rs:115-126`) has early-exit:
```rust
match promise_ref.state() {
    v8::PromiseState::Pending => { sleep...; }
    _ => break,  // ← exits immediately when settled
}
```

The `setup_require` poll loop (`mod.rs:369-374`) does NOT:
```rust
while Instant::now() < deadline {
    isolate.perform_microtask_checkpoint();
    std::thread::sleep(Duration::from_micros(100));  // always sleeps
}
```

### Impact

Every bundle load with `node_builtins: true` now takes 1 second longer than before. The old tight-spin code (`for _ in 0..10_000 { checkpoint(); }`) completed in ~1-5ms. All 4 node-builtins sample tests load bundles at test time — they're now 1s slower each.

The idempotency gap (no early return if `require` already set) amplifies this: N bundles loaded into the same isolate = N seconds of unnecessary sleep.

---

## Solution

Modify `execute_script` to **return the promise value** from JS, then **check its state inside the poll loop** — exactly how `call_render` does it. Requires 0 scope-chain voodoo: `execute_script` returns `v8::Global<v8::Value>`, which wraps the promise, and `Global::open` + `Local::try_from` give us the promise state.

### How execute_script captures the promise

`MainWorker::execute_script` returns `Result<v8::Global<v8::Value>, String>` — the JS expression's result as a V8 handle that outlives the scope chain.

The current setup script:
```js
globalThis.__ssr_require_promise = (async () => {
    const { createRequire } = await import('node:module');
    globalThis.require = createRequire('file:///');
})();
```

Returns `undefined` (assignment expression result). We add a final expression to return the promise:
```js
globalThis.__ssr_require_promise = (async () => {
    const { createRequire } = await import('node:module');
    globalThis.require = createRequire('file:///');
})();
globalThis.__ssr_require_promise;
```

Now `execute_script` returns `Global<Value>` wrapping the Promise, which we can open and poll.

### Poll loop with early-exit

```rust
while Instant::now() < deadline {
    isolate.perform_microtask_checkpoint();

    // Open the Global<Value>, try to downcast to Promise, check state
    let val_ref = require_promise_val.open(isolate);
    if let Ok(promise) = v8::Local::<v8::Promise>::try_from(val_ref) {
        if promise.state() != v8::PromiseState::Pending {
            break;  // ← settled (fulfilled or rejected)
        }
    }

    std::thread::sleep(Duration::from_micros(100));
}
```

The `v8::Local::<v8::Promise>::try_from` pattern already exists in `call_render.rs:88`. No new imports needed.

### Verification retains the `execute_script` check

The post-loop verification stays as-is — it converts "promise rejected" into a clear error message. If the promise rejected, `globalThis.require` is undefined and the verification throws.

---

## Implementation Steps

### [ ] Step 1: Modify setup script to return the promise

**File:** `ext/ssr_deno/src/deno_runtime_wrapper/mod.rs`

In `setup_require`, change the execute_script call to capture the return value and have the script return the promise:

Before:
```rust
    worker
        .execute_script(
            "<ssr-deno:require>",
            r#"
            globalThis.__ssr_require_promise = (async () => {
                const { createRequire } = await import('node:module');
                globalThis.require = createRequire('file:///');
            })();
            "#
            .to_string()
            .into(),
        )
        .map_err(|e| format!("Failed to start require import: {e}"))?;
```

After:
```rust
    let require_promise_val = worker
        .execute_script(
            "<ssr-deno:require>",
            r#"
            globalThis.__ssr_require_promise = (async () => {
                const { createRequire } = await import('node:module');
                globalThis.require = createRequire('file:///');
            })();
            globalThis.__ssr_require_promise;
            "#
            .to_string()
            .into(),
        )
        .map_err(|e| format!("Failed to start require import: {e}"))?;
```

### [ ] Step 2: Add early-exit promise-state check inside poll loop

**File:** `ext/ssr_deno/src/deno_runtime_wrapper/mod.rs`

Replace the current poll loop:
```rust
    let isolate = worker.js_runtime.v8_isolate();
    let deadline = Instant::now() + Duration::from_secs(1);
    while Instant::now() < deadline {
        isolate.perform_microtask_checkpoint();
        std::thread::sleep(Duration::from_micros(100));
    }
```

With:
```rust
    let isolate = worker.js_runtime.v8_isolate();
    let deadline = Instant::now() + Duration::from_secs(1);
    while Instant::now() < deadline {
        isolate.perform_microtask_checkpoint();

        let val_ref = require_promise_val.open(isolate);
        if let Ok(promise) = v8::Local::<v8::Promise>::try_from(val_ref) {
            if promise.state() != v8::PromiseState::Pending {
                break;
            }
        }

        std::thread::sleep(Duration::from_micros(100));
    }
```

### [ ] Step 3: (Optional) Add idempotency guard

**File:** `ext/ssr_deno/src/deno_runtime_wrapper/mod.rs`

At the top of `setup_require`, before the `execute_script` call, add:

```rust
    // If this isolate already had a bundle loaded (node_builtins enabled),
    // globalThis.require is already set — skip the entire setup.
    if worker
        .execute_script(
            "<ssr-deno:require-guard>",
            "typeof globalThis.require !== 'undefined'".to_string().into(),
        )
        .map(|_| ())
        .is_ok()
    {
        let val = worker
            .execute_script(
                "<ssr-deno:require-guard>",
                "typeof globalThis.require !== 'undefined'".to_string().into(),
            )
            .unwrap();
        let guard_ref = val.open(worker.js_runtime.v8_isolate());
        if guard_ref.is_true() {
            return Ok(());
        }
    }
```

**Wait — this approach is wrong.** `execute_script` with a `"true"` or `"false"` string evaluates it as a bare expression that gets discarded. And opening `v8::Global<v8::Value>` from outside a scope chain requires the isolate, which we need to get.

Simpler guard approach using the existing `worker`:
```rust
    // Fast path: if require is already set (e.g., from a prior bundle load
    // in the same isolate), skip the async import + poll loop entirely.
    let isolate = worker.js_runtime.v8_isolate();
    {
        let check_val = worker
            .execute_script(
                "<ssr-deno:require-guard>",
                "typeof globalThis.require !== 'undefined'"
                    .to_string()
                    .into(),
            )
            .map_err(|e| format!("Failed to check require: {e}"))?;
        let check_ref = check_val.open(isolate);
        if check_ref.is_true() {
            return Ok(());
        }
    }
```

But this requires `isolate` to be obtained before the guard check, which means a separate `v8_isolate()` call... Actually, `isolate` from `worker.js_runtime.v8_isolate()` gives the same `&Isolate` each time. We could get it before the guard block.

Actually, the simplest guard: 
```rust
    // Fast path: if require is already set from a previous bundle load
    // into the same isolate, skip the async import + poll loop.
    let check_val = worker
        .execute_script(
            "<ssr-deno:require-guard>",
            "typeof globalThis.require !== 'undefined'"
                .to_string()
                .into(),
        )
        .map_err(|e| format!("Failed to check require: {e}"))?;
    let isolate = worker.js_runtime.v8_isolate();
    let check_ref = check_val.open(isolate);
    if check_ref.is_true() {
        return Ok(());
    }
```

Wait, but `isolate` is obtained AFTER `check_val`, and `check_val.open(isolate)` needs `isolate`. But `v8::Global::open` takes any reference to the Isolate (it uses `AsMut`). The issue is: can we open a Global with an isolate reference obtained after the Global was created?

In `call_render.rs` line 91, the Global is created with `unsafe { &*isolate_raw }` where `isolate_raw` is derived from `isolate` before the scope chain. The Global is tied to a specific isolate. If we call `worker.js_runtime.v8_isolate()` twice, it returns the same `&Isolate` each time (same underlying V8 isolate). So this should work.

Actually, in the call_render flow, the Global is created at line 91 (inside the scope chain with `isolate_raw`), and then opened at line 118 with `isolate` (obtained from `js_runtime.v8_isolate()` at line 24). The `isolate` reference at line 118 is the same exact V8 isolate as the one used to create the Global at line 91. So you CAN create a Global with one reference and open it with another reference to the same isolate.

For the guard, the issue is simpler: the Global returned by `execute_script` is managed by V8's handle scope system. When we call `open` on it, we need a reference to the same isolate. And `worker.js_runtime.v8_isolate()` always returns a reference to the same isolate. So:

```rust
let check_val = worker.execute_script(...)?; // returns Global<Value>
let isolate = worker.js_runtime.v8_isolate(); // get &Isolate ref
let check_ref = check_val.open(isolate);      // open Global with same isolate
```

But wait — `execute_script` borrows `worker` mutably. The returned `Global<Value>` doesn't borrow `worker`. And `worker.js_runtime.v8_isolate()` borrows `worker` immutably. But since `execute_script`'s borrow ended (its result is consumed by `?`), the `worker` borrow is released. So `v8_isolate()` can borrow `worker` immutably with no conflict.

OK this approach works. But it adds complexity and an extra `execute_script` call on every bundle load. Is it worth it?

For the FIRST bundle load in an isolate:
- Without guard: execute_script + poll loop (1ms) + verify execute_script
- With guard: execute_script (guard check, ~1µs, returns false) + execute_script + poll loop + verify

For SUBSEQUENT bundle loads in the same isolate:
- Without guard: execute_script + poll loop (1ms) + verify execute_script
- With guard: execute_script (guard check, ~1µs, returns true) → early return!

The guard adds ~1µs on the first load but saves ~1ms on each subsequent load. For a typical Rails app with 5-10 bundles, this saves ~4-9ms total. Not huge, but it's simple and self-documenting.

I'll include it as Step 3 in the plan.

---

### [ ] Step 4: Update plan files

**File:** `plans/setup-require-improvements.md`

Add a note at the top referencing this follow-up plan as the fix for the early-exit regression. And/or add a "Post-Implementation Notes" section.

Actually, it's cleaner to just reference this plan from the original. I'll add a note to `setup-require-improvements.md`.

---

## Files Changed

| File | Change |
|------|--------|
| `ext/ssr_deno/src/deno_runtime_wrapper/mod.rs` | Capture promise return value from execute_script; add early-exit promise-state check in loop; optional idempotency guard |
| `plans/setup-require-improvements.md` | Add cross-reference to this plan |

## Files NOT Changed

| File | Reason |
|------|--------|
| `docs/ARCHITECTURE.md` | Already updated in previous commit |
| `CHANGELOG.md` | No new user-facing change (the deadline+verify behavior is the same; the fix is internal) |
| `ext/ssr_deno/src/deno_runtime_wrapper/call_render.rs` | Unrelated |
| `ext/ssr_deno/crates/ssr_deno_core/src/lib.rs` | No type changes |
| `lib/ssr/deno.rb` / `sig/ssr/deno.rbs` | No API changes |

---

## Verification

1. `bundle exec rake` passes (Rust compile, cargo test, sample builds, Ruby tests, RuboCop, RBS, SimpleCov 100%)
2. Node builtins integration tests still pass (they exercise the success path)
3. Unnecessary sleep is eliminated: on a warm isolate, the loop breaks on the first iteration (~0µs sleep)
4. Promise rejection is still caught: verification `execute_script` at the end converts it to a clear error
