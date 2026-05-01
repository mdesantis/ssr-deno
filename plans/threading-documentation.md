# Threading Model Documentation

> **Source:** Recommendation #2 from [`memory-performance-analysis.md`](memory-performance-analysis.md)
> **Cross-refs:** [`architecture.md`](architecture.md) (dedicated worker thread design), [`deno_runtime_wrapper.rs`](../ext/ssr_deno/src/deno_runtime_wrapper.rs) (channel-based serialization), [`multiple-isolates.md`](multiple-isolates.md) (isolate pool — supercedes the single-isolate model)

---

## Problem

Users may assume that adding more Puma threads increases SSR throughput, since that's how most Rails database queries scale. In the original architecture (single V8 isolate), adding threads only increased contention on the channel, not throughput. With the isolate pool ([`multiple-isolates.md`](multiple-isolates.md)), threads **do** scale up to the pool size, but beyond that they queue on the pool.

## Approach

Add a "Threading Model" section to [`README.md`](../README.md) that explains:

1. The architecture (isolate pool — N V8 isolates shared across threads)
2. The scaling rule (threads scale up to pool size, workers scale indefinitely)
3. The throughput expectations (table by component tree complexity × pool size)
4. The Ractor story (safe, isolates dispatched round-robin)
5. The render timeout behavior (from the timeout implementation)

---

## Changes

### [`README.md`](../README.md)

Add a new section after the "Rails integration" subsection (line 72) and before "Development" (line 74):

_NOTE: This plan was written before the isolate pool was implemented. The README now reflects the pool architecture directly — see the Configuration section. This plan is kept for historical reference of the threading discussion._

```markdown
### Threading Model

`ssr-deno` uses a **V8 isolate pool** — up to N background threads, each with
its own V8 isolate. Render requests are dispatched in round-robin fashion:

```
┌─────────────────┐
│ Puma Thread 1   │──┐
├─────────────────┤  │    round-robin    ┌────────────┐
│ Puma Thread 2   │──┼──────────────────▶│ Isolate 1   │
├─────────────────┤  │                   ├────────────┤
│ Puma Thread 3   │──┘                   │ Isolate 2   │
└─────────────────┘                      ├────────────┤
                                         │ ...         │
                                         └────────────┘
```

Pool size defaults to `CPU_cores - 1` (capped at 8, min 1).

#### Scaling

| What you scale | Effect on SSR throughput |
|---|---|
| **Puma workers** (processes) | ✅ Linear — each worker has its own isolate pool |
| **Puma threads** | ✅ Scales to pool size — each thread can run on a different isolate |
| **Ractors** | ✅ Safe — isolates are dispatched without locks |

**To scale SSR throughput, increase the pool size or add Puma workers.**

#### Throughput per worker (pool_size = 4)

| Component tree | `renderToString` time | Max req/s per worker |
|---|---|---|
| Simple (10 components) | ~5–15 ms | ~260–800 |
| Medium (50 components) | ~15–40 ms | ~100–260 |
| Complex (200+ components) | ~40–100 ms | ~40–100 |

Under contention (more threads than isolates), requests queue on the pool.
P95 latency increases linearly with queue depth.

#### Ractor safety

The native extension declares `rb_ext_ractor_safe(true)`, so you can call
`SSR::Deno.native_render` from Ractors. Isolates are dispatched round-robin
using a lock-free `AtomicUsize` counter, so Ractors get true parallelism
up to the pool size.

#### Render timeout

If a JavaScript `render` function hangs (infinite loop, deadlock), the isolate
would block indefinitely. `ssr-deno` enforces a **30-second timeout** on all
render calls. When the timeout fires:

- A `SSR::Deno::RenderError` is raised on the calling thread
- The affected isolate remains usable for subsequent renders
- Other isolates in the pool are unaffected

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
