# CI Workflow Speed-Up Plan

## Problem

The CI workflow ([`.github/workflows/ci.yml`](.github/workflows/ci.yml)) consistently fails or times out because:

1. **V8 from source compilation** (`V8_FROM_SOURCE: 'true'`) takes ~3+ hours per job
2. **Debug profile** (`RB_SYS_CARGO_PROFILE: 'dev'`) is slower than release
3. **3 Ruby versions** (3.3, 3.4, 4.0) run in parallel, each compiling V8 independently
4. **No caching** of Rust/V8 build artifacts between runs
5. GitHub's 6-hour workflow limit is hit for Ruby 3.4 and 4.0 jobs

## Constraint: `V8_FROM_SOURCE=true` is mandatory

From [`.env.example`](.env.example:1-11) and [`plans/archived/v8-tls-issue.md`](plans/archived/v8-tls-issue.md):

The native extension is a **cdylib** (`.so`), and V8's thread-local storage uses the `local-exec` model by default, which is incompatible with shared libraries. Building V8 from source with `v8_monolithic_for_shared_library=true` changes the TLS model to `local-dynamic`, producing compatible relocations.

The prebuilt `rusty_v8` binary does **not** include this flag, so **option 1 (prebuilt V8) is not viable** until upstream PR [#1970](https://github.com/denoland/rusty_v8/pull/1970) lands.

## Proposed Solutions (ordered by impact)

### 1. Use `sccache` for V8 C++ compilation caching (highest impact)

**From [`.env.example`](.env.example:24-26):** `sccache` (S3-compatible compiler cache) caches V8's C++ compilation artifacts. Since V8's C++ build is the bulk of the 3-hour compile time, this provides massive speedup across CI runs.

**Setup:**
```yaml
- name: Install sccache
  run: |
    cargo install sccache --locked
    echo "SCCACHE=/home/runner/.cargo/bin/sccache" >> $GITHUB_ENV
```

`sccache` automatically wraps `cc`/`cxx` when `SCCACHE` is set — no code changes needed. On GitHub Actions, it uses the local filesystem cache by default (or can be configured with S3/GCS for shared caching across runs).

**Trade-off:** First run after enabling is still full-build speed. Subsequent runs with cache hits are dramatically faster.

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
