# CI cache budget

Repo exceeded GitHub's fixed 10GB Actions cache cap (peaked 10.26GB with **zero
open PRs**). LRU then evicted 3 of 6 `cargo-target` caches on `main`, so those
legs rebuilt cold, re-saved, evicted others — thrash. Surfaced as `Cache
cleanup` failing with HTTP 429 on the merge of
[PR #7](https://github.com/mdesantis/ssr-deno/pull/7);
[PR #8](https://github.com/mdesantis/ssr-deno/pull/8) fixed that workflow, but
it only reclaims a PR's caches after the PR closes.

Goal: steady state comfortably under the cap with 2+ open PRs, without losing
coverage or slowing CI.

## Baseline (2026-08-15, before any phase)

| ref | kind | count | size |
|---|---|---|---|
| `main` | cargo-target | 7 | 6.01 GB |
| `main` | sccache | 6413 | 3.79 GB |
| `main` | cargo-deps | 1 | 0.25 GB |
| `main` | other (bundler) | 6 | 0.21 GB |
| **total** | | **6425** | **10.26 GB** |

Per open PR, before Phase 1: ~5.6 GB.

## Why it overflows

Three multipliers stack:

1. **Per-PR duplication.** A PR run reads `main`'s cache but saves into
   `refs/pull/<N>/merge`, unreadable by anything else.
2. **Per-commit rotation.** The target key ends in a hash of every `.rs` file,
   and `actions/cache` only skips saving on an *exact* primary-key hit. Run
   `31841046346`: four legs restored via `restore-keys`, then all seven jobs
   saved new entries.
3. **Per-Ruby duplication.** Each ~800MB entry is dominated by
   `release/gn_out/obj/librusty_v8.a` plus the Deno dep graph — byte-identical
   across Ruby 3.3/3.4/4.0.

## The bug underneath

`CARGO_TARGET_DIR` is relative, `RB_SYS_CARGO_TARGET_DIR` absolute. Cargo
resolves a relative value against the CWD, and every cargo call in
`rakelib/cargo.rake` runs with `chdir: 'ext/ssr_deno'`. So `cargo
test`/`clippy`/`fmt`/`llvm-cov` build into `ext/ssr_deno/tmp/cargo-target` —
never cached, rebuilt cold every run on all six legs (`cargo test
ssr_deno_dev_mode` alone: 201s x64 / 526s arm64). Only `rake compile` writes the
cached tree, because rb-sys passes `--target-dir` explicitly.

This reframes sccache: on the release path `actions/cache` keeps cargo from
invoking rustc at all, so sccache is never consulted there. Its ~18% hit rate
(`plans/archived/ci-speedup.md`) is almost entirely serving this shadow build —
it is paying ~3.79GB to partly mitigate the bug. **Do not remove sccache before
the duplication is fixed.**

Also: `cargo install cargo-llvm-cov` runs at workspace root *before* the cache
steps and honours `CARGO_TARGET_DIR`, so its whole dep graph is baked into every
leg's saved entry.

## Phase 1 — PRs restore, never save ✅

Split all four `actions/cache@v6` steps into `actions/cache/restore@v6` plus a
trailing `actions/cache/save@v6` gated on `github.ref == 'refs/heads/main'`.
Keys unchanged, so the effect is cleanly attributable.

Expected: −5.6GB per open PR; PR count stops mattering. PR *reads* also keep
`main`'s entries hot, since eviction is by last access.

**Does not fix:** `main` still mints a new generation on every Rust-touching
merge, so `main` alone stays at ~10GB. The overshoot now evicts the previous
generation rather than live caches — degradation instead of thrash — but there
is no real headroom. That is Phase 2.

**Trade-off:** a PR leg has no fallback of its own. When `main`'s entry is
evicted, every push to that PR pays a cold build (~24m measured, inside the 45m
timeout) until `main` repopulates — slower, not broken, and strictly better
than the old behaviour where the PR's own writes evicted `main`'s entries. This
gets better as Phase 2/3 shrink `main`'s footprint and eviction stops.

### Phase 1 measured result

Merged as `4ea1a4a`. Verified with a throwaway Rust-source commit to force a
key miss: **0 saves** across the PR run (5 legs restored via `restore-keys`, 1
missed outright — all 7 would have written before), and `refs/pull/9/merge`
ended up holding only sccache entries. On the follow-up `main` run exactly 1
save fired, from the one leg whose key genuinely missed — the gate works in
both directions.

`Cache cleanup` then drained 2122 entries from that merge ref to zero.

Post-merge steady state, **still over cap with zero open PRs**:

| ref | kind | count | size |
|---|---|---|---|
| `main` | cargo-target | 7 | 6.01 GB |
| `main` | sccache | 6710 | 3.62 GB |
| `main` | cargo-deps | 1 | 0.25 GB |
| **total** | | **6958** | **10.34 GB** |

## Phase 2 — restructure

Order matters; L3 must land before or with L1.

- **L3 — move Ruby-independent work off the 6-leg matrix.** ✅ New
  `rust-checks` job (x86_64, Ruby 4.0) owns `cargo:fmt`, `cargo:clippy`, all
  `cargo:test:*`, `cargo:coverage`, with `CARGO_TARGET_DIR` absolute so its dev
  tree is a single cacheable directory. New `lint` job (no Rust toolchain, no
  cargo cache) owns `rubocop`, `rubocop:rails`, `rbs`. Legs keep compile,
  samples, Ruby tests, `coverage:check`.
- **L5 — key and size hygiene.** Partly done: `CARGO_INCREMENTAL: 0`
  workflow-wide; `cargo install cargo-llvm-cov` built into a scratch dir and
  removed from the matrix legs entirely; `llvm-cov-target/` pruned before save;
  `CARGO_PROFILE_DEV_DEBUG: 0` on msrv. **Still open:** a rustc-version
  component in the key (`dtolnay/rust-toolchain` exposes `outputs.cachekey`) —
  without it a floating-`stable` bump leaves stale artifacts in the restored
  `deps/` forever, since cargo never GCs them.
- **L1 — drop sccache.** Only after L3 lands and its effect is measured. Frees
  ~3.6GB and ~6700 entries. Measure first: add `sccache --show-stats` to one
  job — the 18% figure is a one-off, not ongoing telemetry, and L3 is expected
  to remove most of what sccache was covering.

## Phase 3 — one target cache per arch

Drop `matrix.ruby` from the target key; gate the save to a canonical leg
(`matrix.ruby == '4.0'`) so non-canonical legs are restore-only. 4.66 → 1.6GB.
Separate PR, independently revertible.

**Safe for the release tree only.** rb-sys exports `RBCONFIG_*` and emits
`cargo:rerun-if-env-changed` for every key it reads, so changing Ruby changes
declared fingerprint inputs → build script re-runs → bindings regenerate →
magnus and the cdylib rebuild, while the Ruby-independent 99% is reused. A
wrong-ABI `.so` fails loudly at `require` in `rake test`.

**Never share the dev/test tree across Ruby legs.** Raw `cargo test -p ssr_deno`
does not go through rb-sys, so `RBCONFIG_*` is unset and rb-sys shells out to
`ruby` from `PATH` — which cargo does not fingerprint. L3 removes the hazard by
confining raw `cargo test` to one job on one Ruby.

`[profile.ci]` is not an option: rb-sys raises on any profile name other than
`dev`/`release`. Use `CARGO_PROFILE_RELEASE_*` env overrides instead.

## Later — `Swatinem/rust-cache`

At a stable checkpoint, evaluate replacing the hand-rolled steps. It already
does most of L5 (prunes workspace-crate artifacts, incremental, unused deps,
`registry/src`, week-old files) and ships `save-if` and `shared-key`. Mapping
would be `workspaces: "ext/ssr_deno -> ../../tmp/cargo-target"`, which needs
verifying against the relative-path bug above.

## Verification

Authoritative number is `gh api repos/mdesantis/ssr-deno/actions/cache/usage`,
not a sum of `gh cache list`.

- Phase 1: on a PR touching a Rust source file, every cache step logs `Cache
  restored from key: …` and no `Cache saved with key` line appears; then
  `gh api "repos/mdesantis/ssr-deno/actions/caches?ref=refs/pull/<N>/merge&per_page=1" --jq '.total_count'`
  shows sccache and bundler entries only, zero `cargo-target-*`.
- Push a second commit to the same PR: leg still restores, duration within a few
  minutes of pre-change warm time.
- After merge: exactly one save per job on the `main` run; `Cache cleanup`
  drains the merge ref to zero; re-measure usage and record real per-entry
  sizes — those numbers decide Phase 2's scope.
