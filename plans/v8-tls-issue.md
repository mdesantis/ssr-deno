# V8 TLS Relocation Issue

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

The fix is two-part — both PRs are required:

1. ✅ **[v8 PR #20](https://github.com/denoland/v8/pull/20)** (denoland/v8) — adds `V8_TLS_USED_IN_LIBRARY` define to V8's `internal_config` GN target. This is the define that actually changes the TLS model from `local-exec` to `local-dynamic` in V8's own `.cc` files. **Merged 2026-05-06.**

2. 🔴 **[rusty_v8 PR #1970](https://github.com/denoland/rusty_v8/pull/1970)** (denoland/rusty_v8) — passes `v8_monolithic_for_shared_library=true` GN arg when building V8. This triggers the condition patched by PR #20. **Still open as of 2026-05-08. No activity since 2026-04-26.**

**Dependency chain:** PR #1970 was blocked on v8 PR #20. That blocker is resolved. The autoroll ("Rolling to V8 14.7.173.23") is now open in rusty_v8 — once it lands and the v8 submodule pointer is bumped, PR #1970 can merge.

**Why the workaround works now:** `V8_FROM_SOURCE=true` applies floated patches from `vendor/rusty_v8/patches/`, which already includes the v8-level TLS patch (the same change as PR #20). This is why `GN_ARGS='v8_monolithic_for_shared_library=true'` works locally despite PR #1970 not being merged upstream yet.

**Monitoring:** Watch [rusty_v8 PR #1970](https://github.com/denoland/rusty_v8/pull/1970) and [crates.io `v8` releases](https://crates.io/crates/v8/versions) for a new version that includes the fix. The signal is: PR #1970 merged + crates.io version bumped past current.

## Cleanup (once upstream fix ships)

Once both PRs are merged and the published crate includes the fix, remove the workaround across these locations:

### 1. `ext/ssr_deno/Cargo.toml`
Remove the `[patch.crates-io]` block (lines 60–61):
```toml
[patch.crates-io]
v8 = { path = "../../vendor/rusty_v8" }
```
Then bump the `v8` dependency version to the fixed crate release.

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
With `V8_FROM_SOURCE` gone, clang/LLVM is no longer needed to build the crate. Audit CI and Dockerfile — `libclang-*-dev`, `clang-*`, `lld-*` and `LIBCLANG_PATH` can be removed if nothing else depends on them.

## Verification After Cleanup

Build without workaround env vars and confirm no TLS relocations survive in the output `.so`:

```bash
# Should return empty — no local-exec TLS relocations
readelf -r target/release/libssr_deno.so | grep -E 'TPOFF|TLSLE'

# Confirm the right TLS reloc types are present (local-dynamic is fine)
readelf -r target/release/libssr_deno.so | grep -E 'TLSLD|DTPOFF|TLSGD'
```

CI should pass on a clean checkout with no `V8_FROM_SOURCE` or `GN_ARGS` set, using only the published crate.
