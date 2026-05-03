# setup_require Idempotency Guard

## Problem

`setup_require` runs on every bundle load with `node_builtins: true`, even when
`globalThis.require` is already set from a prior bundle load into the same
isolate. Each call costs: `execute_script` (async import) + 10ms poll loop +
verification `execute_script`. For N bundles loaded into the same isolate, this
is N × 10ms wasted.

The `setup-require-early-exit-fix.md` plan discussed this extensively
(Step 3, lines 182-282) and concluded it's safe and simple — but never
implemented it.

## Solution

Add a Rust-side idempotency guard at the top of `setup_require`:
`execute_script` to check `typeof globalThis.require !== 'undefined'`, open the
`Global<Value>`, and return early if `is_true()`.

The approach is safe because:
- `Global<Value>::open(isolate)` works with any `&Isolate` reference to the
  same V8 isolate (`worker.js_runtime.v8_isolate()` always returns the same
  `&Isolate`).
- `Global<Value>` from `execute_script` doesn't borrow `worker` — the `?`
  consumes the borrow, and `v8_isolate()` can then borrow `worker` immutably.
- `Local<Value>::is_true()` is a simple boolean check — no `TryFrom` or `cast`
  needed (unlike the failed Promise-state check in the early-exit fix).

## Implementation Steps

### [x] Step 1: Add idempotency guard to `setup_require`

**File:** `ext/ssr_deno/src/deno_runtime_wrapper/mod.rs`

Insert before the existing `worker.execute_script("<ssr-deno:require>", ...)`:

```rust
    // Idempotency guard: skip setup if require is already set from a
    // previous bundle load into the same isolate.
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

Note: `isolate` is obtained after `check_val` because `execute_script` borrows
`worker` mutably. The `?` releases that borrow, and `v8_isolate()` borrows
`worker` immutably — no conflict.

## Files Changed

| File | Change |
|------|--------|
| `ext/ssr_deno/src/deno_runtime_wrapper/mod.rs` | Add idempotency guard before the async import `execute_script` |

## Files NOT Changed

| File | Reason |
|------|--------|
| `ext/ssr_deno/src/deno_runtime_wrapper/call_render.rs` | Unrelated |
| `ext/ssr_deno/crates/ssr_deno_core/src/lib.rs` | No type changes |
| `lib/ssr/deno.rb` / `sig/ssr/deno.rbs` | No API changes |
| `docs/architecture.md` | Implementation detail, not architectural |

## Verification

1. `bundle exec rake` passes (Rust compile, cargo test, sample builds, Ruby
   tests, RuboCop, RBS, SimpleCov 100%)
2. Node builtins integration tests still pass (they load multiple bundles into
   the same isolate — the guard must not break the success paths)
3. Second bundle load's `setup_require` returns early (verified by code review:
   `is_true()` branch returns `Ok(())` before the async import)
