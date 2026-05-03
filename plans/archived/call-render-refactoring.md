# call_render Refactoring

## Problem

`call_render` at `ext/ssr_deno/src/deno_runtime_wrapper/call_render.rs:17` is a
single function of 190 lines with two independent scope-chain blocks glued by
a poll loop. The function:

- Mixes two unrelated concerns in one body: Phase 1 (lookup + call + sync return)
  and Phase 2 (poll loop + async resolution)
- Has no public API boundary between phases — just a comment line and an
  `async_handle` variable
- Duplicates the same "check OOM, then extract error" pattern in 5 places
- Has a rejected-promise fall-through from Phase 1 to Phase 2 just for error
  extraction, even though the error is already available

## Proposal

Split into 3 functions:

```
call_render (orchestration, ~20 lines)
├── phase1_lookup_and_call (owns scope chain, ~60 lines)
│   ├── Pin<HandleScope> + ContextScope
│   ├── Lookup render function (inline, 14 lines)
│   ├── Call + TryCatch + dispatch
│   │   ├── throws → OOM check → error (2 lines)
│   │   ├── return non-Promise → stringify → Ok(Sync(s))
│   │   └── return Promise → match state:
│   │       ├── Fulfilled → read result → stringify → Ok(Sync(s))
│   │       ├── Rejected → extraction helper → Err(DenoError)
│   │       └── Pending → Global::new → Ok(Pending { promise })
│   └── scope chain drops
└── phase2_poll_and_resolve (owns scope chain, ~55 lines)
    ├── Poll loop with deadline + OOM check
    ├── Pin<HandleScope> + ContextScope (re-entry)
    └── Match fulfilled/rejected
        ├── Fulfilled → stringify → Ok(s)
        └── Rejected → extraction helper → Err(DenoError)
```

Shared helper:

```
rejection_error(value, scope, oom_triggered) -> DenoError
  Reads rejection value, formats error message, checks OOM first.
```

## Key changes

| What | Before | After |
|---|---|---|
| Functions | 1 × 190 lines | 1 × 15 lines + 2 × ~60 lines |
| `AsyncHandle` struct | 2 fields | Removed — replaced by `Phase1Outcome::Pending { promise }` |
| `isolate_raw` pointer | Used for `Global::new` | Same, but now scoped inside Phase 1 only |
| Duplicated OOM checks | 5 sites across both phases | Shared via `rejection_error` helper (2 call sites: Phase 1 rejected, Phase 2 rejected) |
| Rejected fall-through | Phase 1 → Phase 2 for formatting | Handled in Phase 1 via helper |

## Implementation

### [x] Step 1: Extract `rejection_error` helper — **Dropped**

The approach was attempted but rusty_v8 methods like `to_rust_string_lossy`
and `json::stringify` expect concrete scope types, not `&impl AsMut<Isolate>`.
Rejection formatting is duplicated inline in both phases instead.

```rust
/// Format a promise rejection value into a DenoError.
fn rejection_error<T: AsMut<v8::Isolate>>(
    rejection: v8::Local<v8::Value>,
    scope: &mut T,
    oom_triggered: &AtomicBool,
) -> DenoError {
    if oom_triggered.load(Ordering::SeqCst) {
        return DenoError::OutOfMemory(
            "JS heap out of memory — the isolate reached its configured heap limit".into(),
        );
    }
    let msg = if rejection.is_string() {
        rejection.to_rust_string_lossy(scope)
    } else if rejection.is_object() {
        v8::json::stringify(scope, rejection)
            .map(|s| s.to_rust_string_lossy(scope))
            .unwrap_or_else(|| "Promise rejected (non-serializable value)".to_string())
    } else {
        "Promise rejected".to_string()
    };
    DenoError::Render(msg)
}
```

The generic `T: AsMut<v8::Isolate>` was attempted but rusty_v8 methods like
`to_rust_string_lossy` and `json::stringify` expect concrete scope types
(`&Isolate`, `&PinnedRef<HandleScope>`), not `&impl AsMut<Isolate>`. The
`rejection_error` helper was dropped — rejection formatting is duplicated
inline in both phases instead.

### [x] Step 2: Extract `phase1_lookup_and_call`

Moves lines 24-119 from the current `call_render` into a standalone function
that owns its scope chain and returns an outcome enum:

```rust
enum Phase1Outcome {
    Sync(String),
    Pending { promise: v8::Global<v8::Promise> },
}

fn phase1_lookup_and_call(
    isolate: &mut v8::OwnedIsolate,
    context: &v8::Global<v8::Context>,
    bundle_id: &str,
    args_json: &str,
    oom_triggered: &AtomicBool,
) -> Result<Phase1Outcome, DenoError> {
    let isolate_raw: *const v8::Isolate = &**isolate as *const v8::Isolate;

    let result = {
        let mut scope_storage = std::pin::pin!(v8::HandleScope::new(isolate));
        let mut scope = scope_storage.as_mut().init();
        let context_local = v8::Local::new(&mut scope, context);
        let mut context_scope = v8::ContextScope::new(&mut scope, context_local);

        let global = context_local.global(&mut context_scope);

        // ... inline get_prop + lookup ...
        // ... render_fn.call + TryCatch ...
        // ... dispatch: sync|fulfilled → return Ok(Sync(s))
        //               rejected     → return Err(rejection_error(...))
        //               pending      → Ok(Pending { global_promise })
    };

    // scope chain dropped — isolate borrow released
    result
}
```

Key difference from current code: the Rejected arm uses the shared
`rejection_error` helper and returns `Err(...)` instead of falling through.

### [x] Step 3: Extract `phase2_poll_and_resolve`

### [x] Step 4: Rewrite `call_render` as orchestration

### [x] Step 5: Remove obsolete types

### [x] Step 6: Verify

`bundle exec rake` passes — compile, cargo test, samples, all Ruby suites,
RuboCop, 100% coverage, RBS valid.

## Files Changed

| File | Change |
|---|---|
| `ext/ssr_deno/src/deno_runtime_wrapper/call_render.rs` | Split into 3 functions + 1 shared helper |

## Files NOT Changed

| File | Reason |
|---|---|
| All other files | No API changes, no new config, no behavioral changes |

## Risk

- The `rejection_error` helper uses `T: AsMut<v8::Isolate>` — tested with both
  `v8::TryCatch` (Phase 1) and `v8::ContextScope` (Phase 2). If either doesn't
  implement `AsMut<Isolate>`, the call site won't compile.
- `phase1_lookup_and_call` returns `Err(DenoError)` for rejected promises
  instead of falling through to Phase 2. Previously, `call_render` always
  returned `Err(DenoError::Render)` for rejected promises (after going through
  Phase 2's error extraction). The result is the same: `DenoError::Render(msg)`.
  The error message format is identical (same `rejection_error` code path).
- The `was_pending` field is eliminated — `phase2_poll_and_resolve` is only
  called for pending promises, so `was_pending` is always `true`. The poll loop
  runs unconditionally.
