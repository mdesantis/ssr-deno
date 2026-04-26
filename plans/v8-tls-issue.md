# V8 TLS Relocation Issue — Investigation Summary

## The Problem

When linking `librusty_v8.a` (v147.4.0) into a **cdylib** (`.so`), the linker fails with:

```
relocation R_X86_64_TPOFF32 against hidden symbol
'_ZN2v88internal18g_current_isolate_E' can not be used when making a shared object
```

This is a **TLS (Thread-Local Storage) model mismatch**. V8's `g_current_isolate` variable uses the `local-exec` TLS model, which is only compatible with executables — not shared libraries.

## Root Cause

### V8's TLS Variable

In [`v8/src/execution/isolate.cc`](https://github.com/v8/v8/blob/main/src/execution/isolate.cc):

```cpp
thread_local Isolate* g_current_isolate_ V8_CONSTINIT = nullptr;
```

This is a thread-local variable that stores the current V8 isolate pointer. On Linux, the default TLS model for this is `local-exec`, which generates `R_X86_64_TPOFF32` relocations. These relocations reference the TLS block at a fixed offset from the thread pointer — an offset that can only be resolved at link time for executables, not shared libraries.

### The Fix: `V8_TLS_USED_IN_LIBRARY`

V8 has a compile-time flag `V8_TLS_USED_IN_LIBRARY` that changes the TLS model from `local-exec` to `local-dynamic`. When this flag is set:

1. In [`v8/src/common/thread-local-storage.h`](https://github.com/v8/v8/blob/main/src/common/thread-local-storage.h): `V8_TLS_LIBRARY_MODE` is set to `1`
2. TLS access goes through a getter function call instead of direct inline access
3. The resulting relocations are `R_X86_64_TLSLD` or `R_X86_64_DTPMOD`, which ARE compatible with shared libraries

### How the v8 Crate Handles This

In [`v8-147.4.0/build.rs`](https://github.com/denoland/rusty_v8/blob/v147.4.0/build.rs) (lines 354-381):

```rust
// Use the shared-library-safe TLS mode by default on Linux so downstream
// cdylibs can link rusty_v8 archives.
let needs_tls_define = target_os == "linux";
// ... injects extra_cflags=["-DV8_TLS_USED_IN_LIBRARY"] into GN args
```

This flag injection **only happens when building V8 from source** (`V8_FROM_SOURCE=true`). When using the prebuilt binary (the default), the flag is **not applied** — the prebuilt binary was supposedly built with this flag by the CI, but our investigation shows otherwise.

## Investigation Findings

### 1. Deno's Approach

The Deno repository ([`/tmp/deno-check/`](https://github.com/denoland/deno)):

- **Builds as an executable**, not a shared library
- Uses `v8 = { version = "147.4.0", default-features = false, features = ["simdutf"] }` — same as us
- Has `[profile.dev.package.v8] opt-level = 1` (V8 needs at least -O1)
- Has `[profile.release.package.v8] opt-level = 3`
- `.cargo/config.toml` does **NOT** set `V8_FROM_SOURCE` — uses prebuilt binaries
- **Never hits this issue** because executables can use `local-exec` TLS

### 2. Prebuilt Binary Analysis

The prebuilt binary at `target/release/gn_out/obj/librusty_v8.a`:

```
nm output for g_current_isolate:
  0000000000000000 B _ZN2v88internal18g_current_isolate_E    (BSS - local-exec TLS)
  0000000000000000 W _ZTWN2v88internal18g_current_isolate_E  (weak TLS wrapper)
                    U _ZN2v88internal18g_current_isolate_E    (undefined references)
```

The `B` (BSS) symbol type confirms `local-exec` TLS model. If `V8_TLS_USED_IN_LIBRARY` had been used, the symbol would appear differently (likely as a defined symbol in `T` or `D` section with `local-dynamic` TLS).

### 3. rusty_v8 CI Configuration

The CI workflow at `.github/workflows/ci.yml` uses `V8_FROM_SOURCE: true` for ALL builds. This means the CI **does** build from source, and the `build.rs` lines 379-381 **should** inject the TLS flag. However, the prebuilt binary for v147.4.0 still has `local-exec` TLS.

**Possible explanations:**
- The CI builds might use `GN_ARGS` that override the injected flag (line 358-377 handles this, but only if `GN_ARGS` contains `extra_cflags=[`)
- The CI might use a different build configuration that bypasses the flag injection
- The prebuilt binary might be from a different build process than the CI workflow suggests
- There might be a bug in the v147.4.0 release where the prebuilt binary wasn't rebuilt after the TLS fix was added to `build.rs`

### 4. `V8_FROM_SOURCE=true` Build Attempts

| Attempt | Result | Reason |
|---------|--------|--------|
| 1st | Failed | Missing `libglib2.0-dev` — installed it |
| 2nd | Failed | Missing vendored `icu_calendar_data` path — the v8 crate's source distribution doesn't include all GN dependencies |

The `V8_FROM_SOURCE` path requires a full Chromium-style build environment with all vendored dependencies (ICU data, etc.), which the `v8` crate's published source package doesn't fully provide.

## Resolution: Successful Build with Proper GN Args

### The `extra_cflags` Problem

The `build.rs` fallback (PR [#1911](https://github.com/denoland/rusty_v8/pull/1911)) uses `extra_cflags=["-DV8_TLS_USED_IN_LIBRARY"]` to inject the define. However, `extra_cflags` is **not a declared top-level GN arg** — it's only an invoker parameter inside toolchain definitions in `build/toolchain/gcc_toolchain.gni`. GN silently accepts unknown args, so the define never reaches the compiler.

You can verify this by grepping the generated `gn_out/obj/v8/v8_base_without_compiler.ninja` after a build — `V8_TLS_USED_IN_LIBRARY` is absent from `defines = ...`.

### The Correct GN Args

The `V8_TLS_USED_IN_LIBRARY` define is gated on **two** real declared GN args in [`v8/BUILD.gn:1232`](https://github.com/denoland/rusty_v8/blob/v147.4.0/v8/BUILD.gn#L1232):

```gn
if (v8_monolithic && v8_monolithic_for_shared_library) {
  defines += [ "V8_TLS_USED_IN_LIBRARY" ]
}
```

Both default to `false`:
- `v8_monolithic` — [`v8/gni/v8.gni:77`](https://github.com/denoland/rusty_v8/blob/v147.4.0/v8/gni/v8.gni#L77)
- `v8_monolithic_for_shared_library` — [`v8/BUILD.gn:379`](https://github.com/denoland/rusty_v8/blob/v147.4.0/v8/BUILD.gn#L379)

Neither is set anywhere in `build.rs` or `.gn` `default_args`.

### Successful Build

Built rusty_v8 v147.4.0 from source with:

```bash
V8_FROM_SOURCE=true GN_ARGS="v8_monolithic=true v8_monolithic_for_shared_library=true" cargo build --release
```

Verification after build:

```
$ readelf -r target/release/gn_out/obj/v8/v8_base_without_compiler/isolate.o | grep TPOFF32
# (no output — zero TPOFF32 relocations)

$ readelf -r target/release/gn_out/obj/v8/v8_base_without_compiler/isolate.o | grep TLS
R_X86_64_TLSLD
R_X86_64_DTPOFF32
R_X86_64_TLSGD
```

The resulting `libssr_deno.so` (46MB) links successfully with zero `R_X86_64_TPOFF32` relocations.

## Upstream Status

### Related Issues and PRs

| # | Status | Description |
|---|--------|-------------|
| [#1706](https://github.com/denoland/rusty_v8/issues/1706) | CLOSED | Original TPOFF32 relocation error report |
| [#1798](https://github.com/denoland/rusty_v8/issues/1798) | OPEN | "rusty_v8 as cdylib" |
| [#1831](https://github.com/denoland/rusty_v8/issues/1831) | CLOSED | GN args not taken into account between runs |
| [#1911](https://github.com/denoland/rusty_v8/pull/1911) | MERGED | "fix: enable linux shared-library-safe v8 tls mode by default" — **broken fix** using `extra_cflags` |
| [#1970](https://github.com/denoland/rusty_v8/pull/1970) | OPEN | "fix: use real GN arg for linux shared-library TLS mode" — **correct fix** switching to `v8_monolithic_for_shared_library=true` |
| [#20](https://github.com/denoland/v8/pull/20) | OPEN | Companion patch on `denoland/v8` to wire `V8_TLS_USED_IN_LIBRARY` into `internal_config` |

### How the Fixes Work Together

1. **`denoland/v8` PR #20**: Adds `V8_TLS_USED_IN_LIBRARY` to V8's `internal_config` (so internal `.cc` files see it) and **drops the `v8_monolithic &&` requirement** from the `:features` config condition — changing `if (v8_monolithic && v8_monolithic_for_shared_library)` to just `if (v8_monolithic_for_shared_library)`.

2. **`denoland/rusty_v8` PR #1970**: Replaces the broken `extra_cflags` injection in `build.rs` with `v8_monolithic_for_shared_library=true`, which is a real declared GN arg.

Once both land, `v8_monolithic_for_shared_library=true` alone will be sufficient — no need for `v8_monolithic=true`.

## Possible Solutions

### Solution A: Wait for upstream fix (Long-term)

Wait for PR #1970 and companion `denoland/v8` PR #20 to land, then update the rusty_v8 dependency.

### Solution B: Use `V8_FROM_SOURCE` with `GN_ARGS` (Current approach — working)

Set `V8_FROM_SOURCE=true` and `GN_ARGS="v8_monolithic=true v8_monolithic_for_shared_library=true"`. This requires cloning the full `rusty_v8` repo (not just the crate from crates.io) to get all vendored deps.

The `[patch.crates-io]` section in `ssr-deno`'s `Cargo.toml` points to the local rusty_v8 checkout:

```toml
[patch.crates-io]
v8 = { path = "/home/maurizio/Sviluppo/rusty_v8" }
```

### Solution C: Linker Workarounds (Not recommended)

- `-Wl,-z,notext`: Allows dynamic relocations in text section — doesn't fix TLS model
- `-Wl,--no-undefined`: Would just make the error more explicit
- These don't solve the fundamental TLS model incompatibility

### Solution D: Build as an Executable Helper (Alternative)

Instead of linking V8 directly into the Ruby C extension (`.so`), build a small Rust **executable** that embeds `deno_core::JsRuntime` and communicates with Ruby via:
- **stdin/stdout JSON protocol** (simple, no TLS issue)
- **Unix domain sockets** (for persistent connection)
- **Shared memory** (for performance)

This completely avoids the TLS relocation issue because executables can use `local-exec` TLS.

### Solution E: Use `deno_runtime` crate's approach (Investigating)

The `deno_runtime` crate (the full Deno runtime, not just `deno_core`) might have a different approach to V8 initialization that avoids the TLS issue. However, `deno_runtime` pulls in many unnecessary dependencies (deno_fs, deno_io, deno_web, deno_fetch, etc.) and is designed for the full Deno CLI.

## Recommendation

**Solution B (V8_FROM_SOURCE with GN_ARGS)** is the current working approach. Once PR #1970 and companion PR #20 land upstream, we can switch back to using the published crate without any special configuration.

If upstream takes too long or the fix introduces regressions, **Solution D (Executable Helper)** remains a viable fallback that avoids the TLS issue entirely.
