# Plan: Rust Unit Tests for `ssr_deno` Native Extension

## Goal

Increase Rust-level test coverage of the [`ssr_deno`](../ext/ssr_deno/src/) crate, focusing on pure-Rust logic that doesn't require a V8 isolate, a Ruby interpreter, or a tokio runtime.

Currently only `resolve_pool_size()` is tested (4 tests in [`lib.rs`](../ext/ssr_deno/src/lib.rs:239-285)).

## Key Constraint: V8 Build

**`cargo test` doesn't currently work.** The vendored [`rusty_v8`](../../vendor/rusty_v8/) crate needs `V8_FROM_SOURCE=true` + custom `GN_ARGS` (see [`.env.example`](../../.env.example:10-11) and [`v8-tls-issue.md`](v8-tls-issue.md)) to build from source, which takes hours. Without those env vars set, the pre-built binding file for the `simdutf` variant is missing:

```
error: couldn't read `.../src_binding_simdutf_release_x86_64-unknown-linux-gnu.rs`
```

This means any `#[cfg(test)]` module inside the `ssr_deno` crate will fail to compile because the crate's `Cargo.toml` declares `deno_runtime` as a dependency, which transitively depends on `v8`.

**Solution:** Move testable pure-Rust logic into a **separate library crate** in a Cargo workspace — one that has zero dependency on `v8`, `deno_runtime`, `tokio`, or `magnus`. This crate (e.g. `ssr_deno_core`) can be compiled and tested independently with plain `cargo test`.

## Proposed Structure

```
ext/ssr_deno/
├── Cargo.toml          # workspace root
├── Cargo.lock
├── extconf.rb
├── crates/
│   └── ssr_deno_core/  # <-- new: pure logic, testable without V8
│       ├── Cargo.toml  #     only depends on std
│       └── src/
│           └── lib.rs  #     DenoError, MAX_ISOLATES, pure functions
├── src/
│   ├── lib.rs                     # existing cdylib
│   ├── deno_runtime_wrapper.rs    # imports from ssr_deno_core
│   ├── sys.rs
│   └── nop_types.rs
└── target/
```

Running tests: `cargo test -p ssr_deno_core`

## Scope: What to Move and Test

### 1. `DenoError` enum + Display + Error impl

Move from [`deno_runtime_wrapper.rs`](../ext/ssr_deno/src/deno_runtime_wrapper.rs:24-45) to `ssr_deno_core`.

**Tests:**
- `BundleLoad(msg)` → Display includes msg
- `WorkerInit(msg)` → Display includes msg
- `WorkerDied(msg)` → Display includes msg
- `BundleNotFound(msg)` → Display includes msg
- `Render(msg)` → Display includes msg
- All variants satisfy `std::error::Error` trait bounds
- `source()` returns `None` for all variants

### 2. `MAX_ISOLATES` constant

Move from [`deno_runtime_wrapper.rs`](../ext/ssr_deno/src/deno_runtime_wrapper.rs:70) to `ssr_deno_core`.

**Tests:**
- Value is `8` (the documented hard cap)

### 3. `validate_pool_size(size)` — extracted from `IsolatePool::new()`

The validation in [`IsolatePool::new()`](../ext/ssr_deno/src/deno_runtime_wrapper.rs:148-170) (size 0 rejected, size > `MAX_ISOLATES` rejected) can't be tested because `new()` spawns real V8 threads. Extract into a pure function `validate_pool_size(size: usize) -> Result<(), DenoError>`.

**Tests:**
- Rejects `size == 0` with `WorkerInit` error
- Rejects `size > MAX_ISOLATES` with `WorkerInit` error
- Accepts `size == 1`
- Accepts `size == MAX_ISOLATES`

### 4. Round-robin counter — extracted from `next_handle()`

The [`next_handle()`](../ext/ssr_deno/src/deno_runtime_wrapper.rs:181-183) uses `fetch_add(1, Relaxed) % len`. Extract into `next_index(counter: &AtomicUsize, len: usize) -> usize`.

**Tests:**
- With 3 slots, 6 calls cycles `[0, 1, 2, 0, 1, 2]`
- With 1 slot, all calls return index 0
- Counter wraps without panic (Relaxed ordering)

### 5. `max_heap_size_mb_checked(mb)` — extracted from `native_set_max_heap_size_mb()`

The [`checked_mul(1024 * 1024)`](../ext/ssr_deno/src/lib.rs:90) overflow check. Move to `ssr_deno_core`.

**Tests:**
- `0` → Ok(0)
- `64` → Ok(67108864)
- `usize::MAX / 1024 / 1024` → Ok (boundary)
- `usize::MAX / 1024 / 1024 + 1` → Err (overflow on 64-bit)
- Error message is descriptive

### 6. `resolve_pool_size()` — already exists

Already tested in [`lib.rs`](../ext/ssr_deno/src/lib.rs:244-284). Move to `ssr_deno_core` (keeping existing tests).

### 7. `Config` struct — pure data, low value

Can be tested in `ssr_deno_core` or left as-is. Low priority.

## What Stays in the Main Crate (NOT testable)

- `IsolatePool` (spawns real V8 threads)
- `IsolateHandle::spawn()` / `worker_thread_main()`
- `build_worker()`, `load_bundle_in_worker()`, `call_render()`
- All `native_*` magnus FFI functions
- `map_render_error()`, error constructors using magnus
- `get_or_init_pool()`, `get_pool()`
- `sys.rs` and `nop_types.rs` (Deno boilerplate, thin delegations)

## Summary

| What | Where | Tests | Deps | `cargo test` works? |
|------|-------|-------|------|---------------------|
| `DenoError` | `ssr_deno_core` | 6 | std only | ✅ Yes |
| `MAX_ISOLATES` | `ssr_deno_core` | 1 | std only | ✅ Yes |
| `validate_pool_size` | `ssr_deno_core` | 4 | std only | ✅ Yes |
| `next_index` | `ssr_deno_core` | 3 | std only | ✅ Yes |
| `max_heap_size_mb_checked` | `ssr_deno_core` | 4 | std only | ✅ Yes |
| `resolve_pool_size` | `ssr_deno_core` | 4 | std only | ✅ Yes |
| IsolatePool, FFI, MainWorker | `ssr_deno` (main crate) | — | v8/ruby | ❌ No |

**Total: ~22 tests, all runnable with `cargo test -p ssr_deno_core` without building V8.**

## Implementation Steps

1. ✅ **Create workspace root** in [`ext/ssr_deno/Cargo.toml`](../ext/ssr_deno/Cargo.toml) — wrap existing cdylib and new `ssr_deno_core` in a `[workspace]`.

2. ✅ **Create `ext/ssr_deno/crates/ssr_deno_core/Cargo.toml`** — no dependencies beyond `std`.

3. ✅ **Create `ext/ssr_deno/crates/ssr_deno_core/src/lib.rs`** — move `DenoError`, `MAX_ISOLATES`, pure functions from `deno_runtime_wrapper.rs` and `lib.rs`. Add tests.

4. ✅ **Update `deno_runtime_wrapper.rs`** — import from `ssr_deno_core` instead of defining locally.

5. ✅ **Update `lib.rs`** — import `max_heap_size_mb_checked` from `ssr_deno_core`; remove old `#[cfg(test)]` module.

6. ✅ **Run `cargo test -p ssr_deno_core`** — verify all tests pass without V8 build.

7. ✅ **Add `cargo:test` rake task to [`Rakefile`](../Rakefile)** — runs `cargo test -p ssr_deno_core` as part of `bundle exec rake` default pipeline.

## Files to Create/Modify

| Action | File |
|--------|------|
| Modify | [`ext/ssr_deno/Cargo.toml`](../ext/ssr_deno/Cargo.toml) (add `[workspace]`) |
| Create | `ext/ssr_deno/crates/ssr_deno_core/Cargo.toml` |
| Create | `ext/ssr_deno/crates/ssr_deno_core/src/lib.rs` |
| Modify | [`ext/ssr_deno/src/deno_runtime_wrapper.rs`](../ext/ssr_deno/src/deno_runtime_wrapper.rs) |
| Modify | [`ext/ssr_deno/src/lib.rs`](../ext/ssr_deno/src/lib.rs) |
