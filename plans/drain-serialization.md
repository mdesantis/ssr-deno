# drain_chunks — double serialization per tick

Status: Pending

## Problem

`drain_chunks` serializes the array of chunks through JSON twice per
event-loop tick:

1. JS: `JSON.stringify(c)` produces a string
2. Rust: `serde_json::from_str::<Vec<String>>(&json_str)` parses it back

For SSR workloads (1-2 chunks per tick at ~50ms intervals), the
overhead is negligible — a few microseconds. This is a documented
optimisation opportunity, not a hot path.

## Analysis

```rust
let Ok(global_val) = worker.execute_script(
    "<ssr-deno:render-drain>",
    "(function() { var c = globalThis.__ssr_chunks; \
     globalThis.__ssr_chunks = []; \
     return c && c.length > 0 ? JSON.stringify(c) : null; })()"
        .to_string().into(),
) else {
    return;
};

// ... extract v8::Value, check null/undefined ...

let json_str = local_val.to_rust_string_lossy(&mut context_scope);

// Drop scope before await boundary
drop(context_scope);
drop(scope);

if let Ok(chunks) = serde_json::from_str::<Vec<String>>(&json_str) {
    for chunk in chunks {
        if chunk_tx.send(chunk).await.is_err() {
            break;
        }
    }
}
```

The double serialization happens because JSON.stringify produces a
JS string, and then serde_json parses it. A direct V8 array walk
would avoid both the `stringify` and the `parse`:

```rust
// Alternative: use JSON.parse + direct V8 iteration
// (still has JSON.stringify in JS, avoids serde parse)
let json_str = local_val.to_rust_string_lossy(&mut context_scope);
if let Ok(chunks) = serde_json::from_str::<Vec<String>>(&json_str) {
```

The serde_json parse is fast (zero-copy for `Vec<String>`? No,
each string is owned). For 1-2 chunks of ~50KB HTML, the parse
time is negligible (~1-5µs).

**Even better alternative:** Iterate the V8 array directly:

```rust
if let Some(arr) = local_val.to_object(&mut context_scope) {
    let v8_arr = v8::Local::<v8::Array>::try_from(arr).ok();
    if let Some(arr) = v8_arr {
        let len = arr.length();
        for i in 0..len {
            if let Some(elem) = arr.get_index(&mut context_scope, i) {
                if let Some(s) = elem.as_value().to_rust_string_lossy(&mut context_scope) {
                    if chunk_tx.send(chunk).await.is_err() {
                        break;
                    }
                }
            }
        }
    }
}
```

But this is a larger refactoring (need ownership of `chunk_tx` for
the send, can't cross await with V8 handles). The current approach
with scope-drop-before-send is cleaner.

**Recommendation:** Leave as-is. The double serialization is not a
hot path (1-2 chunks at 50ms intervals). The fix would add V8 API
complexity for negligible gain. Track as a known-low-priority item.

## Test Strategy

No test needed — not a functional change. The existing chunked render
tests (`test_deno_render_chunks.rb`) continue to pass.

## Verification

- [ ] Close as not-a-hot-path (documented, low priority)
