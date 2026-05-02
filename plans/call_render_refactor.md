# `call_render` refactoring — cleanup after async render

After adding promise polling (feat: async render via V8 microtask polling), `call_render`
grew from ~75 to ~185 lines. This plan tightens the result.

## 1. Phase 2 tail — `let result = { ... }; result` → direct block

The last 2 lines of `call_render`:

```
    let result = {
        // ...
        match promise_ref.state() { ... }
    };
    result
```

Block is already an expression — the local binding is noise. Replace with bare block:

```
    {
        // ...
        match promise_ref.state() { ... }
    }
```

Saves 2 lines, removes redundant name.

## 2. Comment tightening

- Phase 1 header: 4 lines of `═══` ASCII art → 2 lines
- Phase 2 header: 4 lines → 2 lines
- End-of-Phase-1 marker: `// ── scope chain dropped here → isolate borrow released ──────────────` is 80 chars. Trim to `// │ scope chain dropped — isolate borrow released`
- Raw Isolate pointer comment: 6 lines → 3 lines. The borrow conflict explanation belongs in the plan, not inline.
- `// ── Phase 1 (scope chain) / Phase 2 (isolate-only) bridge ─────────────────` section header → `// ── Bridge between scope chain and isolate-only phases ──`

## 3. Bundle traversal — DRY

The 3-step `get → filter → ok_or_else` at lines 470-505 repeats:

```
key = v8::String::new(&mut context_scope, name).unwrap()
val = obj.get(&mut context_scope, key.into())
  .filter(|v| !v.is_undefined() && !v.is_null())
  .ok_or_else(|| BundleNotFound(…))?;
typed = val.try_into().map_err(|_| BundleNotFound(…))?;
```

Extract a local closure inside the Phase 1 block:

```
let get_prop = |obj: Local<Object>, key: &str| -> Result<Local<Value>, DenoError> {
    let k = v8::String::new(&mut context_scope, key).unwrap();
    obj.get(&mut context_scope, k.into())
        .filter(|v| !v.is_undefined() && !v.is_null())
        .ok_or_else(|| DenoError::BundleNotFound(
            format!("Property '{key}' not found on SSR object"))
        )
};
```

Then each step becomes 2 lines (`get_prop` + `try_into`) instead of 5. The
`try_into` error still carries the specific context per step (object name, id).

**Trade-off:** The closure borrows `&mut context_scope`, preventing direct use
of `context_scope` while the closure is alive. The 3 traversal calls happen
before `TryCatch` creation, so the closure can be scoped to a sub-block and
dropped before `TryCatch` is created.

## 4. `collect_heap_stats` (`&mut` → `&`?)

Check if `v8_isolate()` really needs `&mut self`. Current:

```
fn collect_heap_stats(worker: &mut MainWorker) -> Result<String, DenoError> {
    let js_runtime = &mut worker.js_runtime;
```

If `JsRuntime::v8_isolate()` exists as `&self`, this becomes `&MainWorker`.
Minor API win — communicates read-only intent.

## Files changed

| File | Change |
|------|--------|
| `ext/ssr_deno/src/deno_runtime_wrapper/call_render.rs` | Apply refactorings 1-3 above |
| `plans/call_render_refactor.md` | This file — checklist marked on completion |

## Not doing

- **Removing `unsafe`** — V8 scope API prevents it (see main analysis above).
  `PinnedRef<TryCatch<HandleScope>>` implements `AsRef<Isolate>` but NOT
  `PinnedRef<TryCatch<ContextScope<HandleScope>>>` — ContextScope breaks the
  Deref chain.
- **Extracting `resolve_promise` helper** — the original plan considered and
  rejected it. `perform_microtask_checkpoint` is a single call, and the
  result-reading scope chain needs access to `context` and `global_promise`
  which would require passing many parameters.
- **Reordering Phase 1 to avoid raw pointer** — TryCatch must wrap
  `render_fn.call()` to prevent V8 unhandled exception marking, but
  `Global::new(&Isolate, promise)` must happen while the promise Local is
  still alive (before TryCatch drops). These two constraints force the raw pointer.

---

Checklist:

- [x] Refactoring 1: Phase 2 tail — remove `let result` binding
- [x] Refactoring 2: Tighten comments (headers, sections, raw pointer)
- [x] Refactoring 3: Bundle traversal DRY with `get_prop` closure
- [x] Verify: `bundle exec rake` passes (compile + test + lint + coverage 100%)
- [x] Verify: docs/comments are not stale
- [x] Commit with caveman-commit format
