# Performance Future Work

_Extracted from `plans/archived/performance-report.md` on 2026-05-10._

## Chunked render mode performance

`render_chunks` uses a different code path (polling `globalThis.__ssr_chunks` array via `drain_chunks`). Performance characteristics may differ from `render` due to JSON serialization overhead.

## Large payload stress test

Benchmarks use small payload (~30 bytes). Larger payloads (e.g. full page data) would stress the JSON serialization boundary between Ruby ↔ Rust ↔ V8.

## Long-running stability test

Heap leak detection over hours of sustained SSR load. V8 GC behavior under continuous allocation/deallocation cycles.

## GVL release experiment

Moved to `plans/gvl-release-experiment.md`.
