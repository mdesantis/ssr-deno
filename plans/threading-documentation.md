# Threading Model Documentation

> **Source:** Recommendation #2 from [`memory-performance-analysis.md`](memory-performance-analysis.md)
> **Cross-refs:** [`architecture.md`](architecture.md) (dedicated worker thread design), [`deno_runtime_wrapper.rs`](../ext/ssr_deno/src/deno_runtime_wrapper.rs) (channel-based serialization)

---

## Problem

Users may assume that adding more Puma threads increases SSR throughput, since that's how most Rails database queries scale. In reality, the single V8 isolate serializes all renders through a tokio channel with buffer depth 1. Adding threads only increases contention on the channel, not throughput.

## Approach

Add a "Threading Model" section to [`README.md`](../README.md) that explains:

1. The architecture (single V8 isolate per process, shared across threads)
2. The scaling rule (workers scale, threads don't)
3. The throughput expectations (table by component tree complexity)
4. The Ractor story (safe but still serialized)
5. The render timeout behavior (from the timeout implementation)

---

## Changes

### [`README.md`](../README.md)

Add a new section after the "Rails integration" subsection (line 72) and before "Development" (line 74):

```markdown
### Threading Model

`ssr-deno` uses a single V8 JavaScript isolate per Ruby process. All render
requests — from any thread or Ractor — are serialized through a channel to a
dedicated background thread:

```
┌─────────────────┐
│ Puma Thread 1   │──┐
├─────────────────┤  │
│ Puma Thread 2   │──┼──> [channel buffer=1] ──> V8 Isolate
├─────────────────┤  │                          (single-threaded)
│ Puma Thread 3   │──┘
└─────────────────┘
```

#### Scaling

| What you scale | Effect on SSR throughput |
|---|---|
| **Puma workers** (processes) | ✅ Linear — each worker has its own V8 isolate |
| **Puma threads** | ❌ None — all threads share one V8 isolate |
| **Ractors** | ❌ None — Ractors serialize on the same channel |

**To scale SSR throughput, scale Puma workers (processes), not threads.**

#### Throughput per worker

Throughput is capped at `1 / render_time` per worker because renders are
serialized. Typical values:

| Component tree | `renderToString` time | Max req/s per worker |
|---|---|---|
| Simple (10 components) | ~5–15 ms | ~65–200 |
| Medium (50 components) | ~15–40 ms | ~25–65 |
| Complex (200+ components) | ~40–100 ms | ~10–25 |

Under contention (multiple threads rendering simultaneously), requests are
queued. P95 latency increases linearly with queue depth.

#### Ractor safety

The native extension declares `rb_ext_ractor_safe(true)`, so you can call
`SSR::Deno.native_render` from Ractors. However, all Ractors still serialize
on the same V8 isolate — there is no parallelism benefit for SSR from Ractors
in the current architecture.

#### Render timeout

If a JavaScript `render` function hangs (infinite loop, deadlock), the V8
isolate would block indefinitely. `ssr-deno` enforces a **30-second timeout**
on all render calls. When the timeout fires:

- A `SSR::Deno::RenderError` is raised on the calling thread
- The V8 isolate remains usable for subsequent renders
- The hung render's result is discarded when it eventually completes

See [Render Timeout](plans/render-timeout.md) for implementation details.
```

### Placement in README

Insert between the Rails integration example block (ends at line 72) and the "Development" heading (line 74). The section flows naturally after users learn how to use `ssr_render` and before they dive into development setup.

---

## Testing

No code changes — documentation only. Verify with:

```bash
# Visual inspection
grep -n "Threading Model" README.md

# Markdown linting (if available)
bundle exec rake rubocop
```
