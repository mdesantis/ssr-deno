# poll_render_state — String allocation every tick

Status: Pending

## Problem

`poll_render_state` calls `to_rust_string_lossy` on every event-loop tick.
A render with 10 ticks does 10 heap allocations.

## Analysis

Looking at the actual code path:

```rust
let local_val = v8::Local::new(&mut context_scope, &global_val);
if local_val.is_null_or_undefined() {
    return RenderState::Pending;
}
let s = local_val.to_rust_string_lossy(&mut context_scope);
```

The JS poll expression returns `null` for the pending case:

```js
globalThis.__ssr_deno_error
 ? ('E:' + globalThis.__ssr_deno_error)
 : (globalThis.__ssr_deno_result === globalThis.__SSR_DENO_SENTINEL
    ? null
    : ('R:' + JSON.stringify(globalThis.__ssr_deno_result)))
```

`null` is caught by `is_null_or_undefined()` BEFORE reaching
`to_rust_string_lossy`. The allocation only runs when the render
has completed (`R:...`) or errored (`E:...`) — one allocation per
render, not per tick.

**No optimization to make here.** Close as not-a-real-problem.

## Verification

- [x] Confirm allocation is terminal-only (done/error), not per-tick

