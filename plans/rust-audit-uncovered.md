# Rust Audit — Uncovered Items (May 2026)

Status: Pending

Items found during the May 2026 audit that were not covered by the
original [rust-audit-fixes.md](rust-audit-fixes.md) (now Closed) or
its child plans.

---

## A. `op_ssr_push_chunk` — dead code in render.rs

**Location:** `ext/ssr_deno/src/deno_runtime_wrapper/render.rs:31-43`

**Problem:** `op_ssr_push_chunk` is registered in the extension
(`mod.rs:691`) but never invoked. The chunked render path
(`render_chunked.rs:56`) defines `globalThis.__ssr_push_chunk` as a
pure JS function that pushes to `globalThis.__ssr_chunks[]` — Rust drains
the array via `drain_chunks` with `JSON.stringify` + `serde_json::from_str`.

The Deno op was the original chunk delivery mechanism but became dead when
the JS array + drain approach was adopted (avoids exposing `Deno.core.ops`
to user scripts, which is hidden post-bootstrap in deno_runtime 0.255+).

**Impact:** Zero runtime cost (registration in vec is cheap). The function
is compiled but never called. No correctness issue — it is unreachable.

**Fix:** Remove the op function and its registration from the extension.
Verify no JS code references `Deno.core.ops.op_ssr_push_chunk`.

### Implementation

```rust
// Remove from render.rs:
// - The entire op_ssr_push_chunk function (lines 31-43)
// - The doc comment above it (lines 24-30)

// Remove from mod.rs:
// - Line 691: ops: Cow::Owned(vec![render::op_ssr_push_chunk()]),
```

### Test Strategy

Covered by existing chunked render tests (`test_deno_render_chunks.rb`).
No new test needed — removal should not change behavior.

### Verification

- [ ] Remove `op_ssr_push_chunk` from `render.rs`
- [ ] Remove op registration from `mod.rs`
- [ ] `bundle exec rake` — must exit 0

---

## B. `deno_exception_class` — Repeated `Ruby::get()` per call

**Location:** `ext/ssr_deno/src/lib.rs:34-39`

**Problem:** `deno_exception_class` calls `Ruby::get().unwrap()` on every
invocation (8 call sites: lines 25, 44, 51, 57, 61, 65, 69, 73). The
function does module lookup + const_get every time — the result never
changes across the lifetime of the extension.

The `Ruby::get()` call is cheap (~atomic load), but `define_module` +
`const_get` traverses Ruby's constant table per call. On error paths
this is negligible (exceptions are rare), but four of the eight call
sites are in `check_not_initialized` and `map_render_error` which are
not error-only.

**Fix:** Cache each `ExceptionClass` in a `OnceLock<ExceptionClass>`.
Compute on first access, reuse thereafter.

### Implementation

```rust
use std::sync::OnceLock;

fn deno_exception_class(name: &'static str) -> ExceptionClass {
    static CACHE: OnceLock<HashMap<&'static str, ExceptionClass>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| HashMap::new());
    // Still need Mutex or similar for concurrent write during init window...
}
```

**Alternative (simpler):** Use `static` + `OnceLock` per class name.
Eight statics, one per class. Clearer and no HashMap overhead.

```rust
macro_rules! exception_class {
    ($name:ident, $class_name:literal) => {
        static $name: OnceLock<ExceptionClass> = OnceLock::new();
        let ruby = Ruby::get().unwrap();
        *$name.get_or_init(|| {
            ruby.define_module("SSR")
                .and_then(|m| m.define_module("Deno"))
                .and_then(|m| m.const_get($class_name))
                .unwrap_or_else(|_| ruby.exception_runtime_error())
        })
    };
}
```

**Consideration:** This optimization is minor — const_get is not a hot
path. Only implement if it measurably improves initialization time.
Document the opportunity and close.

### Test Strategy

No new test needed — behavior is identical. Existing exceptions tests
continue to pass.

### Verification

- [ ] Implement caching (macro or OnceLock per class)
- [ ] `bundle exec rake` — must exit 0

---

## C. Stale TODO — `OnceLock::get_or_try_init` stabilised

**Location:** `ext/ssr_deno/src/lib.rs` (was line 165)

**Problem:**

```rust
// TODO: replace with OnceLock::get_or_try_init once stabilised (tracking issue #109737).
```

The plan claimed `get_or_try_init` was stabilised in Rust 1.80 (July 2024).
This is **incorrect** — the feature is still unstable as of Rust 1.95 (May 2026)
and tracking issue #109737 remains open.

**Fix:** Removed the stale TODO comment. The manual double-check pattern
remains in place and is correct.

### Test Strategy

No new test needed. Existing pool initialization tests cover this path.

### Verification

- [x] Removed stale TODO comment
- [x] `bundle exec rake` — must exit 0

**Completed.**

---

## D. Naked unwrap in nop_types.rs

**Location:** `ext/ssr_deno/src/nop_types.rs:71`

**Problem:**

```rust
deno_runtime::deno_core::url::Url::parse("file:///dev/null").unwrap(),
```

The URL is a valid constant, so it cannot fail. But a naked `unwrap()`
provides no context if the `url` crate parsing rules change.

**Fix:** Replace `unwrap()` with `expect("valid file URL")` or similar
message.

### Test Strategy

Covered by existing tests that exercise worker creation (which
instantiates NopNpmPackageChecker). No new test needed.

### Verification

- [x] Replace `unwrap()` with `expect("valid file URL")`
- [x] `bundle exec rake` — must exit 0

**Completed.** Fixed during compile error resolution — `from_file_path` with `.expect("Valid file URL")` used instead of `unwrap()`.
