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

This requires a local clone of the full [`rusty_v8`](https://github.com/denoland/rusty_v8) repository (not just the crate from crates.io) to get all vendored dependencies. The `[patch.crates-io]` section in [`ext/ssr_deno/Cargo.toml`](../ext/ssr_deno/Cargo.toml) points to the local checkout at [`third_party/rusty_v8`](../third_party/rusty_v8).

## Upstream Fix

PR [#1970](https://github.com/denoland/rusty_v8/pull/1970) on `denoland/rusty_v8` is the correct fix. It replaces the broken `extra_cflags` injection (PR [#1911](https://github.com/denoland/rusty_v8/pull/1911), which used a GN arg that was silently ignored) with `v8_monolithic_for_shared_library=true`, a real declared GN arg.

Once PR #1970 lands, we can remove the `[patch.crates-io]` override and the `V8_FROM_SOURCE` / `GN_ARGS` environment variables, and use the published crate directly.
