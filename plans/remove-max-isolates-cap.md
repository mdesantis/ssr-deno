# Remove MAX_ISOLATES cap

`ssr_deno_core` currently hard-caps pool at 8 isolates (`MAX_ISOLATES = 8`).
Remove the upper bound. Memory (~20-30 MB per idle isolate) is the real limit
— let users decide.

---

## Changes

### `ext/ssr_deno/crates/ssr_deno_core/src/lib.rs`

| What | Change |
|------|--------|
| `MAX_ISOLATES` constant | Remove. Was `pub const MAX_ISOLATES: usize = 8;` |
| `resolve_pool_size(cfg)` | Remove `clamp(1, MAX_ISOLATES)`. Clamp to `[1, usize::MAX]`. |
| `validate_pool_size(size)` | Only reject 0. Remove `size > MAX_ISOLATES` check. |
| Tests | Remove `max_isolates_is_eight`. Update `resolve_pool_size_clamps_to_max`. |

### `ext/ssr_deno/src/deno_runtime_wrapper/mod.rs`

| What | Change |
|------|--------|
| `IsolatePool::new` | No upper-bound check needed. Allocate exactly `pool_size` workers. |

### `lib/ssr/deno.rb` (Ruby layer)

| What | Change |
|------|--------|
| Pool size validation | Remove hard cap in Ruby-side validation if any. |

## Backward compatibility

- Default `isolate_pool_size = 1` unchanged.
- Existing configs with `pool_size ≤ 8` work identically.
- Users who set `pool_size > 8` now get it, previously got clamped to 8.
- Auto-detect formula (`Etc.nprocessors - 1`) may now produce values > 8 on
  high-CPU machines. Consider whether to cap auto-detect separately.

## Risks

- **Memory.** 8 idle isolates ~160-240 MB. 64 isolates ~1.3-2 GB. Users may
  shoot themselves in the foot. Mitigation: document memory budgeting in README.
- **Thread count.** One OS thread per isolate. High isolate counts may starve
  the Ruby VM. Mitigation: pool of 64 should be fine on 16+ core machines.

## Tasks

- [ ] Remove `MAX_ISOLATES` constant in `ssr_deno_core/src/lib.rs`
- [ ] Update `resolve_pool_size` to clamp only at 1 (no upper bound)
- [ ] Update `validate_pool_size` to only reject 0
- [ ] Update tests (remove max_isolates_is_eight, adjust clamp tests)
- [ ] Remove upper-bound check in `IsolatePool::new` if any
- [ ] Check Ruby layer for any hard cap
- [ ] Run `bundle exec rake` to verify

## Extracted from

Part of the [RactorPool plan](ractor-pool.md). Extracted as standalone step
for focused implementation.
