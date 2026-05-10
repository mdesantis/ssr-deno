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

## Approach

1. Add `rb-sys` as direct dep in `Cargo.toml` (already transitive via Magnus)
2. Split `native_render`: check pool existence (Rust-only) before releasing GVL
3. Call `rb_thread_call_without_gvl` around `dispatch_render` (blocking_recv)
4. Re-acquire GVL, call `map_render_error` (needs Ruby exception classes)
5. Benchmark: Puma workers=0 threads=4 pool=4 vs Bundle baseline

## Scope

**Only `native_render`.** `native_render_chunks` loops with per-iteration
GVL release/re-acquire (too complex for this phase). `native_load_bundle`
is called once per bundle — not worth it.

## Success criterion

≥20% throughput improvement with Puma workers=0 threads=4 pool=4 over
Bundle baseline (no GVL release). Measured via `scripts/throughput.rb`.

## Tasks

- [ ] Add `rb-sys` to `ext/ssr_deno/Cargo.toml`
- [ ] Split `get_pool()` path: check pool existence before GVL release,
      do blocking work after release
- [ ] Wrap `dispatch_render` in `rb_thread_call_without_gvl`
- [ ] Run benchmark: `ruby scripts/throughput.rb --no-ractor-pool --workers 0 --threads 4 --isolate-pool-size 4`
- [ ] Compare vs baseline without GVL release
- [ ] Run `bundle exec rake`
