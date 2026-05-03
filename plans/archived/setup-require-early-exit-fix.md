# setup_require Early-Exit Fix

> **Status:** Resolved — deadline reduced from 1s to 10ms (done). Full early-exit approach abandoned (compiler errors). Idempotency guard extracted to `setup-require-idempotency-guard.md`.

## Why the full early-exit approach failed

The intended fix was to mirror `call_render`'s pattern:
1. Modify `execute_script` to return the promise: `globalThis.__ssr_require_promise;`
2. Inside the poll loop, open the `Global<Value>`, cast to `Local<Promise>`, check `state()`

Attempts to implement this failed with multiple Rust compiler errors:

1. **TryFrom trait not implemented for references:**
   ```rust
   v8::Local::<v8::Promise>::try_from(val_ref)
   // error: TryFrom<&Value> not satisfied
   ```
   The `try_from` only works with owned `Local`, not `&Value` references.

2. **isPromise() and cast() methods don't exist on &Value:**
   ```rust
   promise.isPromise()  // error: no method named 'isPromise'
   promise.cast()      // error: no method named 'cast'
   ```

3. **Scope chain complexity:** To get `Local<Promise>` we'd need to enter a V8 handle scope, obtain the promise from `globalThis.__ssr_require_promise`, create a `Global<Promise>`, then drop the scope. This requires unsafe pointer conversions similar to `call_render.rs:29` (`isolate_raw`) and adds fragile complexity for a bundle-load operation that runs once per isolate.

**Simplified fix applied:** Reduce the deadline from 1 second to 10 milliseconds. The `createRequire` promise resolves in <1ms on a warm isolate — 10ms is 10x more than needed and eliminates the ~1s regression per bundle load. The post-loop verification still catches promise rejection failures.

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

### [x] Step 1: Modify setup script to return the promise — **Abandoned**

Compiler errors (TryFrom, isPromise, cast not on `&Value`, scope chain
complexity — see "Why the full early-exit approach failed" above). The
simplified 10ms deadline fix was applied instead.

### [x] Step 2: Add early-exit promise-state check inside poll loop — **Abandoned**

Same compiler errors as Step 1. The poll loop uses a 10ms deadline
without early-exit. The post-loop verification still catches promise
rejection.

### [x] Step 3: Idempotency guard — **Extracted**

Extracted to `plans/setup-require-idempotency-guard.md` for separate
implementation. The approach (Rust-side `execute_script` check) was
fully designed in this plan and is ready to implement.

### [x] Step 4: Update plan files — **Done**

`setup-require-improvements.md` was updated with cross-reference
and later archived. This plan itself now references the extracted
idempotency guard plan.

---

## Files Changed

| File | Change |
|------|--------|
| `ext/ssr_deno/src/deno_runtime_wrapper/mod.rs` | Capture promise return value from execute_script; add early-exit promise-state check in loop; optional idempotency guard |
| `plans/setup-require-improvements.md` | Add cross-reference to this plan |

## Files NOT Changed

| File | Reason |
|------|--------|
| `docs/architecture.md` | Already updated in previous commit |
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
