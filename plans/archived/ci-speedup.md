# CI Workflow Speed-Up Plan

## Problem

The CI workflow ([`.github/workflows/ci.yml`](.github/workflows/ci.yml)) consistently fails or times out because:

1. **V8 from source compilation** (`V8_FROM_SOURCE: 'true'`) takes ~3+ hours per job
2. **Debug profile** (`RB_SYS_CARGO_PROFILE: 'dev'`) is slower than release
3. **3 Ruby versions** (3.3, 3.4, 4.0) run in parallel, each compiling V8 independently
4. **No caching** of Rust/V8 build artifacts between runs
5. GitHub's 6-hour workflow limit is hit for Ruby 3.4 and 4.0 jobs

## Status: Partially resolved

`V8_FROM_SOURCE=true` constraint lifted — upstream fix shipped in rusty_v8 v149.4.0 (2026-06-12). V8 now comes prebuilt from crates.io; CI no longer builds V8 from source. The 3-hour compile time is gone.

**Applied:** options 1 (sccache), 2 (mold), 3 (release profile), 4 (cargo cache) all active in CI.
- sccache wraps rustc via `RUSTC_WRAPPER=sccache` + `mozilla/sccache-action` (GHA cache backend). Provides ~18% Rust hit rate and catches C/C++ units from Deno crates. Complementary to `actions/cache` — they operate at different granularities (whole target dir vs per-unit content hash), so both are useful on dependency-changing builds.
- mold active via `RUSTFLAGS`
- release profile active via `RB_SYS_CARGO_PROFILE`
- cargo registry + target dir cached via `actions/cache`

Remaining options (5 sequential matrix, 6 larger runner) are low-priority — CI is fast now.

## Original Problem

The CI workflow ([`.github/workflows/ci.yml`](.github/workflows/ci.yml)) consistently failed or timed out because:

1. **V8 from source compilation** (`V8_FROM_SOURCE: 'true'`) took ~3+ hours per job
2. **Debug profile** (`RB_SYS_CARGO_PROFILE: 'dev'`) was slower than release
3. **3 Ruby versions** (3.3, 3.4, 4.0) ran in parallel, each compiling V8 independently
4. **No caching** of Rust/V8 build artifacts between runs
5. GitHub's 6-hour workflow limit was hit for Ruby 3.4 and 4.0 jobs

## Proposed Solutions (ordered by impact)

### 1. Use `sccache` for compilation caching (highest impact)

**Applied.** `sccache` wraps both `rustc` and C/C++ compilers, caching per compilation unit by content hash. Backed by GitHub Actions cache via `SCCACHE_GHA_ENABLED=true`. Deno crates still compile some C++ even with prebuilt V8, so sccache covers both. Complementary to `actions/cache` on target dir — `actions/cache` is coarse (whole dir, keyed by Cargo.lock hash); sccache is fine-grained (per unit). When `actions/cache` misses, sccache saves the units whose inputs didn't change.

### 2. Use `mold` linker

**From [`.env.example`](.env.example:19-22):** `mold` is a modern drop-in linker that is ~5-10× faster than GNU `ld`. The final cdylib is ~94 MB, so linking is a significant portion of the build time.

**Setup:**
```yaml
- name: Install mold linker
  run: sudo apt-get install -yq mold
- name: Set mold as linker
  run: echo "RUSTFLAGS='-C link-arg=-fuse-ld=mold'" >> $GITHUB_ENV
```

**Trade-off:** None. `mold` is a pure speed improvement.

### 3. Switch to release cargo profile

**Current:** `RB_SYS_CARGO_PROFILE: 'dev'`

**Fix:** Change to `RB_SYS_CARGO_PROFILE: 'release'`. Release builds compile faster (less debug info, no optimizations for compile-time deps) and produce smaller binaries.

**Trade-off:** Less debug info if a crash occurs in CI. Acceptable for CI.

### 4. Cache Rust build artifacts

Add a caching step using [`actions/cache`](https://github.com/actions/cache) to persist the `target/` directory and cargo registry between runs:

```yaml
- name: Cache Rust build artifacts
  uses: actions/cache@v4
  with:
    path: |
      ext/ssr_deno/target
      ~/.cargo/registry
      ~/.cargo/git
    key: ${{ runner.os }}-cargo-${{ hashFiles('ext/ssr_deno/Cargo.lock') }}
    restore-keys: |
      ${{ runner.os }}-cargo-
```

This helps with incremental compilation across runs.

### 5. Reduce matrix parallelism

Run Ruby 3.3, 3.4, and 4.0 sequentially instead of in parallel, or limit to a single Ruby version for quick feedback with a scheduled full matrix run.

**Trade-off:** Slower total feedback for the full matrix, but avoids resource contention.

### 6. Use a larger GitHub runner

GitHub's `ubuntu-latest` (standard) runner has limited CPU/memory. Switching to `ubuntu-24.04-8core` or `ubuntu-24.04-16core` (larger runners) would speed up compilation significantly.

**Trade-off:** Higher cost per minute.

## Recommended First Step

**Apply options 1 + 2 + 3** (sccache + mold + release profile). These are all documented in [`.env.example`](.env.example) as known speed-up techniques, require no code changes to the Rust source, and together should cut the 3-hour build down significantly.

If still too slow, add **option 4** (caching) and consider **option 5** (sequential matrix).

## Verification

After applying changes, trigger a manual workflow run and verify:
- [ ] All 3 Ruby versions complete within 30-60 minutes
- [ ] `samples:build` succeeds (Deno is now installed)
- [ ] Tests pass
- [ ] RuboCop passes
- [ ] RBS validation passes
