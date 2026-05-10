# Watchdog — Long-lived actor replacing per-render thread spawn

_Extracted from `plans/archived/rust-future-work.md` on 2026-05-10._

## Problem

Every `begin_render` call spawns and joins an OS thread (`watchdog.rs:36-51`).
For typical SSR latency (>10ms) this is acceptable, but at high concurrency
(large pool × many requests/sec) the spawn/join overhead accumulates.

## Solution

One long-lived watchdog actor per isolate. The actor receives
`(deadline, cancel_token)` messages and manages timers internally, eliminating
per-render thread creation.

## Tasks

- [ ] Implement watchdog actor with channel-based deadline/cancel messaging
- [ ] Attach one actor per isolate at worker thread init (not per render)
- [ ] Remove per-render `Watchdog::spawn` from `begin_render`
- [ ] Benchmark throughput improvement under high concurrency (pool ≥ 8, 1000+ req/s)
- [ ] Remove `// TODO` from `pool.rs:145` if unrelated, or verify no new TODOs needed
- [ ] Run `bundle exec rake`
