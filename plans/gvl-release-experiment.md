# GVL Release Experiment

_Extracted from `plans/perf-future-work.md` on 2026-05-10._

## Problem

Every `native_render` FFI call holds the GVL. Magnus does not release it during
`blocking_recv()` on the channel. This means only one Ruby thread can be waiting
on the FFI boundary at a time — multiple isolates provide no throughput benefit
for thread-based Puma.

## Hypothesis

Wrapping the FFI call in `magnus::blocking` would release the GVL, allowing
other Ruby threads to enter the FFI boundary concurrently. If this works,
thread-based Puma could use multiple isolates for parallel SSR without Ractors.

## Tasks

- [ ] Identify the FFI calls that block on channel recv (`native_render`, `native_render_chunks`)
- [ ] Wrap blocking section in `magnus::blocking` (or `rb_thread_call_without_gvl`)
- [ ] Run multi-thread benchmark (e.g. Puma workers=0 threads=4) with pool_size=4
- [ ] Compare throughput vs Bundle baseline (no GVL release)
- [ ] If successful, also wrap `native_load_bundle` and `native_render_chunks`
- [ ] Run `bundle exec rake`
