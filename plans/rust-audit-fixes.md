# Rust Audit Fixes

Status: Pending

## Optimizations

### 1. `render.rs` / `render_chunked.rs` — 90% duplicate event-loop logic

Both files share identical:
- Watchdog setup/teardown
- OOM check loop
- Timeout check loop
- Script template construction (bundle_id, args_json injection)
- `execute_script` error dispatch (BundleNotFound vs Render)

Only differences: chunked runs `drain_chunks` and cleanup, buffered does not.
Extract shared logic into a render skeleton function that takes a per-tick
callback.

### 2. `poll_render_state` — allocates `String` every 50ms tick

`to_rust_string_lossy` per poll call. For a render with 10 ticks, 10 allocs.
Use `v8::String::WriteUtf8` into a reusable buffer or check prefix via
`local_val.to_detail_string` to avoid allocation.

### 3. `drain_chunks` — double serialization per tick

JS `JSON.stringify` → Rust `serde_json::from_str`. Each tick serializes then
deserializes. For 1-2 chunks/tick this is negligible, but direct v8 array
iteration via `get_array_length` + index lookups would eliminate both allocs.

### 4. `setup_require` — 50µs busy-sleep burns CPU

Poll loop spins at ~20kHz (`100ms / 50µs = 2000 iter`). Exponential backoff
or Condvar-style blocking would reduce wakeups.

### 5. `SCRIPT_NAMES` — `Mutex<Option<HashMap>>` could be `OnceLock<Mutex<HashMap>>`

The `Option` is `None` only before first use, then `Some` forever. Extra branch
every access. `OnceLock<Mutex<HashMap>>` eliminates the Option layer.

## Correctness

### 6. `watchdog.rs` — `expect` on thread spawn can panic

```rust
.expect("failed to spawn watchdog thread")
```

OS thread creation failure (rare but real under memory pressure) causes process
abort. Return `Result` instead — fallback to no-watchdog or bubble error.

### 7. `render.rs` — OOM vs timeout priority ordering

In the event loop, OOM is checked first, then timeout. In the `execute_script`
error handler, the same order is used. This is correct (OOM is more specific)
but the implicit assumption should be documented.

## Bug plans (extracted)

- [render-global-cleanup.md](render-global-cleanup.md) — missing JS global cleanup after buffered render
- [poll-sentinel-guard.md](poll-sentinel-guard.md) — `poll_render_state` corrupt sentinel edge case
- [channel-send-error.md](channel-send-error.md) — `drain_chunks` sends to closed channel silently
