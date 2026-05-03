# setup_require Poll Loop Improvements

## Problem

The `setup_require` function (`ext/ssr_deno/src/deno_runtime_wrapper/mod.rs:370`) has two issues:

1. **Hard-coded 10,000 iteration poll loop** — Same issue that was fixed in `call_render`. Runs at bundle-load time (not render time), but still uses an arbitrary iteration limit instead of a time-based deadline.

2. **Silent failure** — After the poll loop, it returns `Ok(())` regardless of whether the `createRequire` promise actually resolved. If the import fails, `globalThis.require` is undefined and bundles fail later with confusing errors.

---

## Implementation Steps

### [ ] Step 1: Add deadline-based poll loop to setup_require

**File:** `ext/ssr_deno/src/deno_runtime_wrapper/mod.rs`

- Apply a hard-coded deadline (e.g., 1 second) + sleep pattern for consistency with the `call_render` fix
- Replace `for _ in 0..10_000` with a time-based loop:
  ```rust
  let deadline = Instant::now() + Duration::from_secs(1);
  while Instant::now() < deadline {
      isolate.perform_microtask_checkpoint();
      // check if require is defined
      // break if settled
      std::thread::sleep(Duration::from_micros(100));
  }
  ```

### [ ] Step 2: Add promise state check after poll loop

**File:** `ext/ssr_deno/src/deno_runtime_wrapper/mod.rs`

After the poll loop, check if the `createRequire` promise actually resolved:

```rust
// After the poll loop, check if the require import actually resolved
let check = r#"
    if (globalThis.require === undefined) {
        throw new Error("createRequire failed - globalThis.require is undefined");
    }
    globalThis.require;
"#;
match eval_js(isolate, check) {
    Ok(_) => Ok(()),
    Err(e) => Err(DenoError::BundleLoad(format!(
        "setup_require failed: {}", e
    ))),
}
```

This ensures that if the import fails, the error is reported immediately at bundle load time rather than failing later with confusing errors.

---

## Files Changed

| File | Change |
|------|--------|
| `ext/ssr_deno/src/deno_runtime_wrapper/mod.rs` | Replace hard-coded loop with deadline-based poll + sleep, add promise state check after loop |

---

## Files NOT Changed

| File | Reason |
|------|--------|
| `lib/ssr/deno.rb` | No API changes |
| `ext/ssr_deno/src/lib.rs` | No core type changes |
| `ext/ssr_deno/src/deno_runtime_wrapper/call_render.rs` | Already fixed separately (see `async-render-polling-improvements.md`) |

---

## Verification

1. Bundle load with invalid require path → error at `Bundle.new` time (not later)
2. Bundle load with valid require path → works as before
3. Poll loop uses time-based deadline instead of hard-coded iteration count
4. `bundle exec rake` passes (Rust compile, cargo:test, Ruby tests, RuboCop, RBS)
