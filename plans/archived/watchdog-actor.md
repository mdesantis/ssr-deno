# Watchdog — Long-lived actor replacing per-render thread spawn

_Extracted from `plans/rust-future-work.md` on 2026-05-10._

**Status:** Won't fix.

Per-render thread spawn/join costs ~50µs. Only measurable on minimal-bundle
microbenchmarks (0.1ms render → 50% overhead). Real SSR workloads (React et al.)
see <5% impact. Not worth the complexity of a long-lived actor.

See comment at `src/deno_runtime_wrapper/render.rs:45` for context.
