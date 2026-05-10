# Rust Future Work — Optimization & Reliability

_Extracted from `plans/archived/rust-codebase-review.md` on 2026-05-08._

## Watchdog — Moved to `plans/watchdog-actor.md`

---

## ~~Dead isolate replacement on partial broadcast failure~~

**Status:** Won't fix — TODO comment at `pool.rs:145` is sufficient documentation.
The pool runs gracefully with fewer isolates after a worker dies. A full
restart/health-check strategy would add complexity disproportionate to the
risk (worker death is extremely rare in practice).

**Source:** `src/deno_runtime_wrapper/pool.rs:145`
