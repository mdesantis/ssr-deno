# Rust Future Work — Optimization & Reliability

_Extracted from `plans/archived/rust-codebase-review.md` on 2026-05-08._

## Watchdog — Replace per-render thread spawn with long-lived actor

**Source:** `src/deno_runtime_wrapper/watchdog.rs:36-51`

Every `begin_render` call spawns and joins an OS thread. For typical SSR latency (>10ms)
this is acceptable, but at high concurrency (large pool × many requests/sec) the
spawn/join overhead accumulates.

**Future option:** one long-lived watchdog actor per isolate. The actor receives
`(deadline, cancel_token)` messages and manages timers internally, eliminating per-render
thread creation.

---

## Dead isolate replacement on partial broadcast failure

**Source:** `src/deno_runtime_wrapper/mod.rs:336-355`

`load_bundle` broadcasts to all isolates. If a worker dies mid-broadcast (`blocking_send`
returns `Err`), isolates 0..N-1 got the bundle and the dead isolate didn't. Round-robin
will dispatch to the partially-loaded isolates (success), but the dead worker is
permanently excluded. No isolate replacement mechanism exists.

Not fixable without a restart/health-check strategy. A `// TODO` comment exists at the
broadcast error site.
