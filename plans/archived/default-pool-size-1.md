# Plan: Default pool size → 1

**Status:** Completed

**Rationale:** Performance report (`plans/archived/performance-report.md`) confirms
threads don't scale (GVL serialization). Auto-detect wastes ~10MB/isolate
with zero throughput benefit for the dominant use case (thread-based Rails).

---

## File changes

### 1. `ext/ssr_deno/crates/ssr_deno_core/src/lib.rs`

- `Config::default()`: `isolate_pool_size: 0` → `1`
- Doc comment: remove "0 = auto-detect from CPU count"
- `resolve_pool_size()`: remove the `if cfg.isolate_pool_size > 0` branch.
  Just `cfg.isolate_pool_size.clamp(1, MAX_ISOLATES)`. If user passes `0`,
  it clamps to `1`.
- Tests: update `assert_eq!(cfg.isolate_pool_size, 0)` → `1`, remove
  auto-detect test case

### 2. `lib/ssr/deno/rails/railtie.rb`

- Line 12: Remove `Rails.env.production? ? nil : 1` — now always `1` by default
- Line 27: Keep `if config.ssr_deno.isolate_pool_size` so explicit user
  overrides still work

### 3. `lib/ssr/deno.rb`

- Doc comment on `isolate_pool_size=` setter: remove "0 = auto-detect" language
- Same for the `apply_env_var_defaults` section

---

## Documentation audit

- `CHANGELOG.md`: add Unreleased entry noting breaking default change
- `README.md`: update `isolate_pool_size = 4` example, remove "0 = auto-detect"
- `docs/architecture.md`: update config descriptions
- `plans/memory-performance-analysis.md`: update auto-detect references
- `plans/archived/performance-report.md`: already consistent (recommends pool=1)

---

## No changes needed

- **Tests** — all explicitly set `isolate_pool_size` to specific values (1, 2)
- **RBS signatures** — type signature unchanged (`Integer → void`)

---

## Scope

| File | Type |
|------|------|
| `ssr_deno_core/src/lib.rs` | Default, logic, tests |
| `rails/railtie.rb` | Remove env-based default |
| `lib/ssr/deno.rb` | Doc comments |
| `CHANGELOG.md` | Entry |
| `README.md` | Examples |
| `docs/architecture.md` | Descriptions |
| `plans/memory-performance-analysis.md` | References |
