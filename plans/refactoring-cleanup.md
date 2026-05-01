# Refactoring & Cleanup

> **Source:** Review of accumulated technical debt after multiple iterations
> (pool implementation, config changes, docs updates).
> **Cross-refs:** [`lib.rs`](../ext/ssr_deno/src/lib.rs), [`deno_runtime_wrapper.rs`](../ext/ssr_deno/src/deno_runtime_wrapper.rs), [`deno.rb`](../lib/ssr/deno.rb),
> [`test_deno.rb`](../test/ssr/test_deno.rb)

---

## Summary

Nine issues identified, ordered by confidence and impact. The first five are
clear wins (low risk, visible improvement). The last four are lower priority.

---

## 1. Duplicated `MAX_ISOLATES` Constant

**Location:** [`lib.rs`](../ext/ssr_deno/src/lib.rs:10) and [`deno_runtime_wrapper.rs`](../ext/ssr_deno/src/deno_runtime_wrapper.rs:70)

**Problem:** Both files define `const MAX_ISOLATES: usize = 8;`. One validates
the cap in `resolve_pool_size()` (lib.rs), the other validates in
`IsolatePool::new()` (deno_runtime_wrapper.rs). If the constants drift, one
validator silently disagrees with the other.

**Fix:** Define `MAX_ISOLATES` once in `deno_runtime_wrapper.rs` and make it
`pub`. Import it in `lib.rs`:

```rust
// deno_runtime_wrapper.rs — change line 70 to:
pub const MAX_ISOLATES: usize = 8;

// lib.rs — add import:
use deno_runtime_wrapper::MAX_ISOLATES;
// Remove the local const MAX_ISOLATES: usize = 8; (line 10)
```

**Risk:** Trivial. Compiler catches any mismatch immediately.

---

## 2. Stale Comment: "OnceLock" in Test Error Handling

**Location:** [`test_deno.rb`](../test/ssr/test_deno.rb:17,26)

**Problem:** Both `test_set_max_heap_size_mb` and `test_set_isolate_pool_size`
have comments saying:

```ruby
# May raise JsRuntimeInitializationError if another test already
# initialized the runtime (OnceLock). We accept either outcome —
```

The runtime guard is now `INITIALIZED: OnceLock<()>`, not a single
`OnceLock<Config>`. The error handling reason is still correct (one test may
init the pool before another's config setter runs), but the `OnceLock`
reference in the comment is misleading — the actual mechanism is now
`Mutex<Config>` + `INITIALIZED` guard.

**Fix:** Update comments to reflect the current guard mechanism:

```ruby
# May raise JsRuntimeInitializationError if another test already
# initialized the pool (INITIALIZED OnceLock guard). We accept either
# outcome — the purpose is coverage of the accessor and the native method.
```

**Risk:** None (comment-only).

---

## 3. Misleading Parameter Name: `per_isolate_heap_mb`

**Location:** [`deno_runtime_wrapper.rs`](../ext/ssr_deno/src/deno_runtime_wrapper.rs:145-148)

**Problem:** The `IsolatePool::new` parameter is named `per_isolate_heap_mb`,
which implies the heap budget is divided per-isolate. In reality, each isolate
gets the full `max_heap_size_mb` — we intentionally do NOT divide the heap
(because `max_heap_size_mb` is a per-isolate V8 CreateParams constraint).

This naming is a leftover from the original plan that divided the heap. It
creates cognitive dissonance when someone reads the code: "if it's per-isolate,
why is lib.rs passing the full value?"

**Fix:** Rename to `max_heap_size_mb`:

```rust
// In IsolatePool::new signature:
pub fn new(size: usize, max_heap_size_mb: usize) -> Result<Self, DenoError> {

// In IsolateHandle::spawn signature:
pub fn spawn(index: usize, max_heap_size_mb: usize) -> Result<Self, DenoError> {

// In worker_thread_main signature:
fn worker_thread_main(
    mut rx: tokio::sync::mpsc::Receiver<WorkerMsg>,
    init_tx: mpsc::SyncSender<Result<(), String>>,
    max_heap_size_mb: usize,
) {
```

Update the following call sites:

- `get_or_init_pool()` in [`lib.rs`](../ext/ssr_deno/src/lib.rs:153) — variable is
  already named `per_isolate_mb`, rename to `max_heap_size_mb` there too.
- The comment on line 148: `pub fn new(size, per_isolate_heap_mb)` → update.

**Risk:** Mechanical rename. No behavioral change.

---

## 4. `INIT_LOCK` Comment Stale

**Location:** [`lib.rs`](../ext/ssr_deno/src/lib.rs:117-119)

```rust
// ---------------------------------------------------------------------------
// Pool initialization (double-checked locking)
// ---------------------------------------------------------------------------
```

**Problem:** "Double-checked locking" normally refers to a volatile + mutex
pattern for lazy initialization of a raw pointer. Here, we have a
`OnceLock<IsolatePool>` (which is atomically checked on read) + a
`Mutex<()>` (which prevents duplicate expensive work during init). The
section label is technically inaccurate — `OnceLock` handles the atomic
first-check, and the mutex is only for the init path, not a true DCLP pattern.

**Fix:** Rewrite the section header to accurately describe the actual
mechanism:

```rust
// ---------------------------------------------------------------------------
// Pool initialization (OnceLock + init mutex)
//   OnceLock provides lock-free reads after init.
//   INIT_LOCK prevents duplicate pool creation during the init window.
// ---------------------------------------------------------------------------
```

**Risk:** None (comment-only).

---

## 5. `INIT_LOCK` vs `POOL` Naming Inconsistency

**Location:** [`lib.rs`](../ext/ssr_deno/src/lib.rs:12-13)

```rust
static POOL: OnceLock<IsolatePool> = OnceLock::new();
static INIT_LOCK: Mutex<()> = Mutex::new(());
```

**Problem:** The naming convention is inconsistent. `POOL` is named by what it
contains (`IsolatePool`). `INIT_LOCK` is named by what it does (locks during
initialization). When scanning the file, it's not obvious that `INIT_LOCK`
guards the `POOL`'s initialization. If the module had multiple static locks,
which one does `INIT_LOCK` guard?

**Fix:** Rename to `POOL_INIT_LOCK` to make the association explicit:

```rust
static POOL: OnceLock<IsolatePool> = OnceLock::new();
static POOL_INIT_LOCK: Mutex<()> = Mutex::new(());
```

Update all references:

- Line 141: `let _guard = POOL_INIT_LOCK.lock().unwrap();`
- Section comment (issue #4)

**Risk:** Mechanical rename. No behavioral change.

---

## 6. `#[allow(dead_code)]` on `IsolatePool::size()`

**Location:** [`deno_runtime_wrapper.rs`](../ext/ssr_deno/src/deno_runtime_wrapper.rs:173-175)

**Problem:** `IsolatePool::size()` is marked `#[allow(dead_code)]` because
nothing calls it. This is a smell — either the method has a planned use
(heap metrics, monitoring), or it should be removed.

**Options:**

a) **Remove it** — simplest, no dead code.
b) **Keep with a doc comment** explaining its intended future use (e.g.,
   "will be used by heap_stats_all for per-isolate metrics reporting").
c) **Use it somewhere** — e.g., in `native_render` as a debug log, or
   expose a Ruby-accessible pool size getter.

**Recommendation:** Option (b) — keep with a clearer doc comment. The
method will be needed by the heap metrics implementation (planned in
[`v8-heap-metrics.md`](v8-heap-metrics.md)). This avoids churn of
re-adding it later.

---

## 7. `map_render_error` Catch-All

**Location:** [`lib.rs`](../ext/ssr_deno/src/lib.rs:74-81)

```rust
fn map_render_error(e: DenoError) -> Error {
    match e {
        DenoError::WorkerDied(msg) => js_runtime_worker_error(msg),
        DenoError::BundleNotFound(msg) => bundle_not_found_error(msg),
        DenoError::Render(msg) => render_error(msg),
        other => js_runtime_worker_error(other.to_string()),
    }
}
```

**Problem:** The catch-all `other =>` would map `BundleLoad` and `WorkerInit`
errors (which shouldn't occur during render) to `JsRuntimeWorkerError`.
This is semantically imprecise. A better approach is an exhaustive match
or a `DenoError::Render`-only conversion, since `map_render_error` is only
called from `native_render`.

**Fix:** Replace with an exhaustive match that explicitly states what each
variant maps to:

```rust
fn map_render_error(e: DenoError) -> Error {
    match e {
        DenoError::WorkerDied(msg) => js_runtime_worker_error(msg),
        DenoError::BundleNotFound(msg) => bundle_not_found_error(msg),
        DenoError::Render(msg) => render_error(msg),
        DenoError::BundleLoad(msg) => js_runtime_initialization_error(msg),
        DenoError::WorkerInit(msg) => js_runtime_initialization_error(msg),
    }
}
```

Or, since `BundleLoad` and `WorkerInit` should be unreachable during
render, use `unreachable!()` for those arms. But that risks panics if a
bug introduces them. The explicit mapping to initialization error is
safer.

**Risk:** Low — these error paths are unreachable in practice, but the
fix makes the code self-documenting.

---

## 8. `deno_exc` Cryptic Function Name

**Location:** [`lib.rs`](../ext/ssr_deno/src/lib.rs:46)

```rust
fn deno_exc(name: &'static str) -> ExceptionClass {
```

**Problem:** `exc` is not a standard abbreviation. `deno_exception` or
`deno_exception_class` would be clearer. The function looks up a Ruby
exception class by name inside the `SSR::Deno` module — its name should
reflect that.

**Fix:** Rename to `deno_exception_class` or just inline the calls (it's
only 6 lines and referenced in 4 places). Keeping it as a function is
fine, but rename for clarity:

```rust
fn deno_exception_class(name: &'static str) -> ExceptionClass {
```

Update all 4 call sites:
- Line 37: `deno_exc("JsRuntimeInitializationError")`
- Line 54: `Error::new(deno_exc(...), ...)`
- Line 58: `Error::new(deno_exc(...), ...)`
- Line 62: `Error::new(deno_exc(...), ...)`
- Line 66: `Error::new(deno_exc(...), ...)`
- Line 70: `Error::new(deno_exc(...), ...)`

Wait, line 54-70 uses `deno_exc` inside the helper functions
(`js_runtime_initialization_error`, etc.), not directly. The direct call
is only at line 37. Let me verify...

Actually, looking at the code:
- Line 37: `deno_exc("JsRuntimeInitializationError")` — inside `check_not_initialized`
- Lines 54-72: The helper functions (`js_runtime_initialization_error`, `js_runtime_not_initialized_error`, etc.) all call `deno_exc` internally.

So `deno_exc` is called in 6 places: once in `check_not_initialized` and
once in each of the 5 helper error functions.

**Risk:** Mechanical rename.

---

## 9. Missing Test Coverage for `resolve_pool_size`

**Location:** [`lib.rs`](../ext/ssr_deno/src/lib.rs:124-134)

**Problem:** `resolve_pool_size()` is only tested indirectly through
functional tests (a default pool of N isolates works). The auto-detect
logic (CPU count, cap at MAX_ISOLATES, saturating_sub for Ruby core) has
no direct unit test. If someone refactors the logic, they could break
edge cases (1-core machine, 0-core value from `available_parallelism`).

**Fix:** Add Rust unit tests for `resolve_pool_size`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_pool_size_explicit() {
        let cfg = Config { isolate_pool_size: 4, max_heap_size_mb: 64 };
        assert_eq!(resolve_pool_size(cfg), 4);
    }

    #[test]
    fn test_resolve_pool_size_clamps_to_max() {
        let cfg = Config { isolate_pool_size: 99, max_heap_size_mb: 64 };
        assert_eq!(resolve_pool_size(cfg), MAX_ISOLATES);
    }

    #[test]
    fn test_resolve_pool_size_minimum() {
        // isolate_pool_size=0 triggers auto-detect; min(available_parallelism-1, 8)
        // On a 1-core CI runner this would return 1. We can't predict the env,
        // but we can test that it's at least 1 and at most MAX_ISOLATES.
        let cfg = Config { isolate_pool_size: 0, max_heap_size_mb: 64 };
        let size = resolve_pool_size(cfg);
        assert!(size >= 1 && size <= MAX_ISOLATES,
            "pool size {size} out of range [1, {MAX_ISOLATES}]");
    }
}
```

**Note:** These tests would live in `lib.rs` (inline `#[cfg(test)] mod tests`)
since `resolve_pool_size` is a private function. They require the `Config`
struct to be visible in the test module (it's already `pub` would need to be
at least `pub(crate)` — actually it's not `pub` at all since it's a private
struct at the module level. Let me verify...

Looking at line 21: `struct Config {` — it's private (module-level without `pub`).
Since `resolve_pool_size(cfg: Config)` takes `Config` by value, and the test
module is `#[cfg(test)] mod tests` inside `lib.rs`, the test has access to
private members. So this should work.

**Risk:** Low — tests-only change.

---

## Prioritization & Effort

| # | Task | Effort | Risk | Value |
|---|------|--------|------|-------|
| 1 | Deduplicate `MAX_ISOLATES` | ~2 min | None | Consistency |
| 2 | Fix stale test comments | ~2 min | None | Accuracy |
| 3 | Rename `per_isolate_heap_mb` → `max_heap_size_mb` | ~5 min | None | Clarity |
| 4 | Fix `INIT_LOCK` section comment | ~1 min | None | Accuracy |
| 5 | Rename `INIT_LOCK` → `POOL_INIT_LOCK` | ~2 min | None | Clarity |
| 6 | `size()` dead_code — add doc comment or remove | ~1 min | None | Housekeeping |
| 7 | Exhaustive `map_render_error` match | ~3 min | Low | Correctness |
| 8 | Rename `deno_exc` → `deno_exception_class` | ~3 min | None | Readability |
| 9 | Unit tests for `resolve_pool_size` | ~10 min | Low | Test quality |

Items 1–6 are quick wins that can be done in a single commit.
Items 7–8 are also low-risk but touch error mapping.
Item 9 is a pure test addition.

---

## Recommended Order

```
Commit 1: Clean up constants, names, and comments
  - 1. Deduplicate MAX_ISOLATES
  - 2. Fix test comments
  - 3. Rename per_isolate_heap_mb → max_heap_size_mb
  - 4. Fix INIT_LOCK section comment
  - 5. Rename INIT_LOCK → POOL_INIT_LOCK
  - 6. Add doc comment to size() or remove dead_code annotation

Commit 2: Error mapping clarity
  - 7. Exhaustive map_render_error match
  - 8. Rename deno_exc → deno_exception_class

Commit 3: Testing
  - 9. Unit tests for resolve_pool_size
```
