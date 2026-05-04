# Rust Audit Fixes

Status: Pending

## Optimizations

### 1. `render.rs` / `render_chunked.rs` — event-loop duplication
→ [archived/render-core-extraction.md](archived/render-core-extraction.md) ✅ implemented

### 2. `poll_render_state` — String alloc every tick
→ [poll-string-alloc.md](archived/poll-string-alloc.md) ✅ closed (terminal-only, not a real problem)

### 3. `drain_chunks` — double serialization per tick
→ [drain-serialization.md](archived/drain-serialization.md) ✅ closed (low priority, serialization overhead negligible)

### 4. `setup_require` — 50µs busy-sleep burns CPU
→ [require-backoff.md](require-backoff.md) — exponential backoff proposal.

### 5. `SCRIPT_NAMES` → `OnceLock` ✅ implemented

## Correctness

### 6. `watchdog.rs` — `expect` on thread spawn can panic
→ [watchdog-spawn-result.md](archived/watchdog-spawn-result.md) ✅ implemented

### 7. OOM vs timeout priority ordering ✅ implemented

## Bug plans (extracted)

- [render-global-cleanup.md](archived/render-global-cleanup.md) — missing JS global cleanup after buffered render ✅ implemented
- [poll-sentinel-guard.md](archived/poll-sentinel-guard.md) — `poll_render_state` corrupt sentinel edge case ✅ implemented
- [channel-send-error.md](archived/channel-send-error.md) — `drain_chunks` sends to closed channel silently ✅ implemented
