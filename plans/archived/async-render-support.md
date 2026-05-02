# Async Render Support — Promise Handling in `call_render`

## Problem

[`call_render`](../ext/ssr_deno/src/deno_runtime_wrapper/call_render.rs) only handles sync function return. Frameworks with async SSR APIs fail:

| Framework | SSR API | Returns | Needs async? |
|-----------|---------|---------|-------------|
| React 19 (`renderToString`) | Sync | `string` | ❌ (works now) |
| Vue 3 (`renderToString`) | Async | `Promise<string>` | ✅ Needs this |
| Svelte 5 (`svelte/server render`) | Sync | `{html, css}` | ❌ (works now) |
| React 19 (`renderToPipeableStream`) | Async callback | Stream | ✅ (future: streaming SSR) |

## Approach

Modify [`call_render`](../ext/ssr_deno/src/deno_runtime_wrapper/call_render.rs) to detect `v8::Promise` return and poll V8 microtask queue until settlement.

### Detection + Polling (inlined in `call_render`)

After `render_fn.call()` returns `Some(val)`:

```
if val is a v8::Promise and state == Pending:
    → move promise into v8::Global<v8::Promise>
    → drop the V8 scope chain (release isolate borrow)
    → loop: isolate.perform_microtask_checkpoint()
            check promise_ref.state() via Global::open(isolate)
    → Fulfilled: re-create HandleScope, JSON.stringify result
    → Rejected: extract message, return Err
    → Pending: continue (up to MAX_POLLS)
else if val is a v8::Promise (already settled):
    → resolve + JSON.stringify (same scope)
else:
    → existing behavior (JSON.stringify)
```

### Key design decision — inlined, not a separate function

The original plan proposed a `resolve_promise` helper taking `impl v8::GetIsolate`. However, `v8::GetIsolate` is `pub(crate)` in the vendored [`rusty_v8`](../vendor/rusty_v8/src/scope.rs:282) and therefore inaccessible from the `ssr_deno` crate. Two alternatives were considered:

1. **Use `v8::Isolate::from_raw_isolate_ptr`** — allows calling `perform_microtask_checkpoint` via a raw pointer across scope drops, but the `TryCatch` lifetime makes borrowing complex.

2. **Use `v8::Global<v8::Promise>` + drop scope chain** — create a `Global` handle (survives scope drops), tear down the scope chain, poll the isolate directly, then re-create scopes when reading the result. This is the adopted approach.

The polling loop is therefore inlined directly in `call_render`:

```rust
if let Ok(promise) = v8::Local::<v8::Promise>::try_from(result) {
    if promise.state() == v8::PromiseState::Pending {
        let global_promise = v8::Global::new(isolate, promise);
        drop(tc);
        drop(context_scope);
        drop(scope_storage);

        for poll in 0..MAX_POLLS {
            unsafe { isolate.perform_microtask_checkpoint(); }
            let promise_ref = global_promise.open(isolate);
            match promise_ref.state() { /* Fulfilled / Rejected / Pending */ }
        }
    }
}
```

### Why `perform_microtask_checkpoint` instead of `poll_event_loop`

- `v8::OwnedIsolate::perform_microtask_checkpoint()` is a **public** method ([`vendor/rusty_v8/src/isolate.rs:1673`](../vendor/rusty_v8/src/isolate.rs:1673)). No `GetIsolate` trait needed.
- `deno_core::JsRuntime::poll_event_loop` requires `&mut JsRuntime` and a `Context`, and the borrow conflicts with the V8 scope chain already active.
- Microtask checkpoint is sufficient for pure promise settlement (no I/O ops needed during SSR render).

### Key constraints

1. **Scope chain must be dropped before polling** — the `v8::HandleScope` borrows `&mut isolate`, so it must be dropped before calling `isolate.perform_microtask_checkpoint()`. The `v8::Global<Promise>` bridges the gap.

2. **Timeout** — existing timeout on `reply_rx.recv_timeout` in [`block_on_render`](../ext/ssr_deno/src/deno_runtime_wrapper/mod.rs) still applies. If promise takes >500ms (default), `recv_timeout` fires and caller gets timeout error.

3. **`MAX_POLLS` safety valve** — prevents infinite loop if V8 microtask queue doesn't settle. Set to 10_000; at ~1µs per checkpoint this is effectively infinite, but prevents unbounded CPU if a promise never settles.

---

## Changes

| File | Change |
|------|--------|
| [`ext/ssr_deno/src/deno_runtime_wrapper/call_render.rs`](../ext/ssr_deno/src/deno_runtime_wrapper/call_render.rs) | Inline async promise polling in `call_render` using `v8::Global<v8::Promise>` + `isolate.perform_microtask_checkpoint()` |
| [`test/fixtures/async-immediate-bundle.js`](../test/fixtures/async-immediate-bundle.js) | New fixture: async function returning string |
| [`test/fixtures/async-resolve-bundle.js`](../test/fixtures/async-resolve-bundle.js) | New fixture: `Promise.resolve()` |
| [`test/fixtures/async-reject-bundle.js`](../test/fixtures/async-reject-bundle.js) | New fixture: `Promise.reject()` |
| [`test/fixtures/async-chained-bundle.js`](../test/fixtures/async-chained-bundle.js) | New fixture: chained `.then()` |
| [`test/ssr/test_deno_async_render.rb`](../test/ssr/test_deno_async_render.rb) | New test file: sync, async-function, Promise.resolve, Promise.reject, chained promise |
| [`sig/ssr/deno.rbs`](../sig/ssr/deno.rbs) | No changes — no new public API |

---

## Cross-refs

- Required by: [`plans/archived/new-samples.md`](new-samples.md) (Vue SSR sample)
- Depends on: `v8::OwnedIsolate::perform_microtask_checkpoint()` (available in current `rusty_v8`)
- Constraint: `v8::GetIsolate` is `pub(crate)` in [`vendor/rusty_v8/src/scope.rs:282`](../vendor/rusty_v8/src/scope.rs:282) — cannot be used from external crates
