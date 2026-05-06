# Remove MAX_ISOLATES cap

`ssr_deno_core` currently hard-caps pool at 8 isolates (`MAX_ISOLATES = 8`).
Remove the upper bound. Memory (~20-30 MB per idle isolate) is the real limit
— let users decide.

Default `isolate_pool_size = 1` stays unchanged.

---

## Scope

### Code changes

| File | Change |
|------|--------|
| `ssr_deno_core/src/lib.rs:54` | Remove `pub const MAX_ISOLATES: usize = 8;` |
| `ssr_deno_core/src/lib.rs:93-103` | `validate_pool_size`: keep only `size == 0` rejection. Remove `size > MAX_ISOLATES` check. |
| `ssr_deno_core/src/lib.rs:128` | `resolve_pool_size`: `clamp(1, MAX_ISOLATES)` → `max(1)` |
| `ssr_deno_core/src/lib.rs:228` | Delete test `max_isolates_is_eight` |
| `ssr_deno_core/src/lib.rs:297-301` | Delete test `validate_pool_size_rejects_over_max` |
| `ssr_deno_core/src/lib.rs:303-311` | Rename `validate_pool_size_accepts_max` → `validate_pool_size_accepts_large`. Test with `64` instead of `MAX_ISOLATES`. |
| `ssr_deno_core/src/lib.rs:332-333` | Rename `resolve_pool_size_clamps_to_max` → `resolve_pool_size_does_not_clamp_large`. Test that `99` stays `99`. |
| `mod.rs:28` | Delete stale comment `// MAX_ISOLATES is available...` |
| `lib/ssr/deno.rb:38` | Remove `max 8` from `@param size` doc |

### Stale docs

| File | Stale text | Fix |
|------|-----------|-----|
| `docs/architecture.md:18` | `(up to 8 isolates` | `(round-robin)` |
| `docs/architecture.md:67` | `N (max 8)` | `N` |
| `docs/compatibility.md:177` | `max 8` | remove |
| `plans/memory-performance-analysis.md:31` | `configurable up to 8` | `configurable, no upper bound` |
| `plans/memory-performance-analysis.md:213` | `(up to MAX_ISOLATES=8)` | remove |
| `plans/memory-performance-analysis.md:439` | `max 8` | remove |
| `plans/memory-performance-analysis.md:460` | `MAX_ISOLATES (8)` | remove |
| `plans/performance-report.md:241` | `pool cap of 8 isolates` | remove |
| `plans/performance-report.md:311-312` | `MAX_ISOLATES = 8` / `Capped at` | remove |
| `plans/performance-report.md:470` | `Increase MAX_ISOLATES cap` | mark `[x]` |

### Auto-detect

`bench/performance.rb` uses `(Etc.nprocessors - 1).clamp(1, 8)` for its auto
pool size. This is a benchmark script default, not library code. Keep the
`clamp(1, 8)` — it's just a demo heuristic for the bench tool. The library's
auto-detect (via `resolve_pool_size`) doesn't clamp to 8 anymore — it should
not clamp at all. User `isolate_pool_size = 0` means "use 1" (the default).

## Backward compatibility

- Default `isolate_pool_size = 1` unchanged.
- Configs with `pool_size ≤ 8` work identically.
- Users who set `pool_size > 8` now get it, previously clamped to 8.

## Risks

- **Memory.** 8 idle isolates ~160-240 MB. 64 isolates ~1.3-2 GB.
- **Thread count.** One OS thread per isolate. High counts starve Ruby VM.

## Completed

Implemented in commit `7d94cc2`. All code and doc changes done. `bundle exec rake`
passes (100% coverage, no failures, RuboCop clean).

## Tasks

- [x] Default `isolate_pool_size = 1` unchanged
- [x] Remove MAX_ISOLATES constant + update validate/resolve/tests
- [x] Clean up mod.rs comment, Ruby param doc
- [x] Update stale docs (architecture, compatibility, plans)
- [x] Mark performance-report.md Future Work as done
- [x] Run `bundle exec rake` to verify
