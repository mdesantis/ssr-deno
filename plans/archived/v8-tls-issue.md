# V8 TLS Relocation Issue

> **Status: RESOLVED** — `rusty_v8 v149.4.0` shipped the fix (2026-06-12). Workaround removed in this repo on 2026-06-14. See [Cleanup](#cleanup) for what was changed.

## The Problem

When linking `librusty_v8.a` (v147.4.0) into a **cdylib** (`.so`), the linker fails with:

```
relocation R_X86_64_TPOFF32 against hidden symbol
'_ZN2v88internal18g_current_isolate_E' can not be used when making a shared object
```

This is a **TLS (Thread-Local Storage) model mismatch**. V8's `g_current_isolate` variable uses the `local-exec` TLS model, which is only compatible with executables — not shared libraries.

## Root Cause

In [`v8/src/execution/isolate.cc`](https://github.com/v8/v8/blob/main/src/execution/isolate.cc):

```cpp
thread_local Isolate* g_current_isolate_ V8_CONSTINIT = nullptr;
```

On Linux, the default TLS model for this is `local-exec`, which generates `R_X86_64_TPOFF32` relocations. These relocations reference the TLS block at a fixed offset from the thread pointer — an offset that can only be resolved at link time for executables, not shared libraries.

V8 has a compile-time flag `V8_TLS_USED_IN_LIBRARY` that changes the TLS model from `local-exec` to `local-dynamic`, producing compatible relocations (`R_X86_64_TLSLD`, `R_X86_64_DTPOFF32`).

### arm64

The relocation error above is x86_64-specific. arm64 uses different TLS relocation types (`R_AARCH64_TLSLE_*`) but the same root cause applies — `local-exec` TLS is incompatible with shared libraries on both architectures. The same `v8_monolithic_for_shared_library=true` GN arg fixes both. All workaround steps below apply equally to arm64 builds.

## Workaround

Build V8 from source with the shared-library-safe TLS mode enabled:

```bash
export V8_FROM_SOURCE=true
export GN_ARGS='v8_monolithic=true v8_monolithic_for_shared_library=true'
export LIBCLANG_PATH=/usr/lib/llvm-21/lib   # Dockerfile uses llvm-21; CI uses llvm-19 — match installed version
bundle exec rake compile
```

This requires a local clone of the full [`rusty_v8`](https://github.com/denoland/rusty_v8) repository (not just the crate from crates.io) to get all vendored dependencies. The `[patch.crates-io]` section in [`ext/ssr_deno/Cargo.toml`](../ext/ssr_deno/Cargo.toml) points to the local checkout at [`vendor/rusty_v8`](../vendor/rusty_v8).

The submodule is pinned at commit `80e204d` (`v0.44.2-694-g80e204d`), which carries the v8-level TLS patch equivalent to [v8 PR #20](https://github.com/denoland/v8/pull/20). [`bin/setup`](../bin/setup) applies [`vendor/rusty_v8.diff`](../vendor/rusty_v8.diff) on top of that commit to fix a GN toolchain issue (`rustc_wrapper_inputs`).

## Upstream Fix

The fix required two changes:

1. ✅ **[v8 PR #20](https://github.com/denoland/v8/pull/20)** (denoland/v8) — adds `V8_TLS_USED_IN_LIBRARY` define to V8's `internal_config` GN target. This is the define that actually changes the TLS model from `local-exec` to `local-dynamic` in V8's own `.cc` files. **Merged 2026-05-06.**

2. ✅ **[rusty_v8 commit `9e52070`](https://github.com/denoland/rusty_v8/commit/9e52070db3bfe4782e7dba1187429713f9303713)** (PR #2008, denoland/rusty_v8) — passes `v8_monolithic_for_shared_library=true` GN arg when building V8. This triggers the condition patched by PR #20. PR #1970 was closed in favour of this direct fix. **Merged 2026-06-12. Released as [`v149.4.0`](https://github.com/denoland/rusty_v8/releases/tag/v149.4.0).**

**Status as of 2026-06-14:** Both fixes are in the published crate. The workaround can be removed. Current project pins `v149.1.0` (yanked) via the `vendor/rusty_v8` path override — upgrade target is `v149.4.0` or later (latest: `v150.0.0`).

**Why the workaround worked:** `V8_FROM_SOURCE=true` applied floated patches from `vendor/rusty_v8/patches/`, which included the v8-level TLS patch (equivalent to PR #20). `GN_ARGS='v8_monolithic_for_shared_library=true'` then triggered it, working around the missing upstream crate fix.

## Cleanup

The upstream fix shipped in `v149.4.0` (2026-06-12). Remove the workaround across these locations:

### 1. `ext/ssr_deno/Cargo.toml`
Remove the `[patch.crates-io]` block (lines 60–61):
```toml
[patch.crates-io]
v8 = { path = "../../vendor/rusty_v8" }
```
Then bump the `v8` dependency version to `149.4.0` or later (currently `149.1.0`, which is yanked). Run `cargo update -p v8` after removing the patch to let Cargo resolve from crates.io.

### 2. `.github/workflows/ci.yml`
Remove the global env vars (lines 18–19):
```yaml
V8_FROM_SOURCE: 'true'
GN_ARGS: 'v8_monolithic=true v8_monolithic_for_shared_library=true'
```

### 3. `Dockerfile`
Remove three `ENV` lines and the `sed` patch-apply step:
```dockerfile
RUN sed -i '/inputs = rustc_wrapper_inputs/d' \
    vendor/rusty_v8/build/toolchain/gcc_toolchain.gni

ENV GN_ARGS='v8_monolithic=true v8_monolithic_for_shared_library=true'
ENV LIBCLANG_PATH=/usr/lib/llvm-21/lib
...
ENV V8_FROM_SOURCE=true
```
Also remove the `COPY vendor/ vendor/` line (vendor dir will be gone).

### 4. `bin/setup`
Remove the patch-apply block (lines 8–13):
```bash
if ! git -C vendor/rusty_v8/build apply ../../../vendor/rusty_v8.diff 2>/dev/null; then
  if git -C vendor/rusty_v8/build apply --reverse --check ../../../vendor/rusty_v8.diff 2>/dev/null; then
    echo "rusty_v8 patch already applied"
  ...
```

### 5. `vendor/` directory
Remove both artifacts:
```bash
git rm -r vendor/rusty_v8
git rm vendor/rusty_v8.diff
git submodule deinit vendor/rusty_v8
```
Also remove the `[submodule "vendor/rusty_v8"]` entry from `.gitmodules`.

### 6. Clang/LLVM toolchain dependency
`libclang` is still required — `libsqlite3-sys` uses `bindgen` which needs it regardless of V8. Only the V8-specific packages can go:
- Dockerfile: remove `ninja-build`, `python3`, `libglib2.0-dev`; keep `clang-*`, `lld-*`, `libclang-*-dev`, `LIBCLANG_PATH`
- CI: replace the full custom LLVM repo + clang-19 install with `libclang-dev` from the default Ubuntu apt (system clang is sufficient for bindgen)

## Verification After Cleanup

Build without workaround env vars and confirm no TLS relocations survive in the output `.so`:

```bash
# Should return empty — no local-exec TLS relocations
readelf -r target/release/libssr_deno.so | grep -E 'TPOFF|TLSLE'

# Confirm the right TLS reloc types are present (local-dynamic is fine)
readelf -r target/release/libssr_deno.so | grep -E 'TLSLD|DTPOFF|TLSGD'
```

CI should pass on a clean checkout with no `V8_FROM_SOURCE` or `GN_ARGS` set, using only the published crate.
