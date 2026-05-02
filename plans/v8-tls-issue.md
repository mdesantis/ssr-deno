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

## Workaround

Build V8 from source with the shared-library-safe TLS mode enabled:

```bash
export V8_FROM_SOURCE=true
export GN_ARGS='v8_monolithic=true v8_monolithic_for_shared_library=true'
export LIBCLANG_PATH=/usr/lib/llvm-21/lib
bundle exec rake compile
```

This requires a local clone of the full [`rusty_v8`](https://github.com/denoland/rusty_v8) repository (not just the crate from crates.io) to get all vendored dependencies. The `[patch.crates-io]` section in [`ext/ssr_deno/Cargo.toml`](../ext/ssr_deno/Cargo.toml) points to the local checkout at [`vendor/rusty_v8`](../vendor/rusty_v8).

## Upstream Fix

The fix is two-part — both PRs are required:

1. **[v8 PR #20](https://github.com/denoland/v8/pull/20)** (denoland/v8) — adds `V8_TLS_USED_IN_LIBRARY` define to V8's `internal_config` GN target. This is the define that actually changes the TLS model from `local-exec` to `local-dynamic` in V8's own `.cc` files.

2. **[rusty_v8 PR #1970](https://github.com/denoland/rusty_v8/pull/1970)** (denoland/rusty_v8) — passes `v8_monolithic_for_shared_library=true` GN arg when building V8. This triggers the condition patched by PR #20.

**Dependency chain:** PR #1970 is **blocked on v8 PR #20** landing and being rolled into the next `*-lkgr-denoland` autoroll. Without PR #20, the `v8_monolithic_for_shared_library` GN arg exists but does nothing — it's not wired to any define in V8's `internal_config`.

**Why the workaround works now:** `V8_FROM_SOURCE=true` applies floated patches from `vendor/rusty_v8/patches/`, which already includes the v8-level TLS patch (the same change as PR #20). This is why `GN_ARGS='v8_monolithic_for_shared_library=true'` works locally despite the PRs not being merged upstream.

Once both PRs are merged and the published crate includes the fix, we can remove the `[patch.crates-io]` override and the `V8_FROM_SOURCE` / `GN_ARGS` environment variables, and use the published crate directly.
