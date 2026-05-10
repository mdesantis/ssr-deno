# GVL Release Experiment

_Extracted from `plans/perf-future-work.md` on 2026-05-10._

## Problem

Every `native_render` FFI call holds the GVL. Magnus does not release it during
`blocking_recv()` on the channel. Only one Ruby thread can wait on the FFI
boundary at a time — multiple isolates provide no throughput benefit for
thread-based Puma.

## Hypothesis

Calling `rb_thread_call_without_gvl` during the blocking `recv()` would let
other Ruby threads enter the FFI boundary concurrently. Thread-based Puma
could then use multiple isolates for parallel SSR without Ractors.

## Constraint

Ractor-mode only. Extension is marked `rb_ext_ractor_safe(true)` —
GVL release is meaningless inside a Ractor (each Ractor has its own GVL).
This experiment is for thread-based concurrency only.

## Approach (implemented)

1. Declared `rb_thread_call_without_gvl` as `extern "C"` in `lib.rs` (symbol linked from libruby, no dep needed)
2. Split `native_render`: call `get_pool()` with GVL held, then release GVL around the blocking `dispatch_render`
3. Packed args in `Box<RenderArgs>` → leaked to raw ptr → consumed by `render_worker` extern C fn → returns `Box<RawRenderResult>`
4. Re-acquire GVL, call `map_render_error`
5. Benchmark: 5,118 → 12,182 req/sec (2.38x, well above ≥20% threshold)

## Scope

**Only `native_render`.** `native_render_chunks` loops with per-iteration
GVL release/re-acquire (too complex for this phase). `native_load_bundle`
and `native_heap_stats` not changed (called infrequently).

## Success criterion ✅

**Exceeded.** 2.38x throughput improvement with Puma workers=0 threads=4 pool=4
(threshold was ≥20%). React SSR measured 4.2x improvement.

## Tasks

- [x] Declare `rb_thread_call_without_gvl` as `extern "C"` in `lib.rs`
- [x] Implement `RenderArgs` + `RawRenderResult` + `render_worker` extern fn
- [x] Wrap `dispatch_render` in `rb_thread_call_without_gvl`
- [x] Update `assert_thread_not_parallel` → `assert_thread_parallel` in test helpers
- [x] Update stale doc comments (GVL serialization no longer true)
- [x] Run benchmark: 5,118 → 12,182 req/sec
- [x] Run `bundle exec rake` — all pass, 100% coverage
