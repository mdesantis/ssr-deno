# Rust Codebase Review — ext/ssr_deno

_Reviewed: 2026-05-07_

## Bug 1 — Same path, different bundle ID silently fails

**File:** `src/deno_runtime_wrapper/mod.rs:544`  
**Severity:** High

`loaded_paths` tracks by `bundle_path`. If two bundle IDs point to the same file:

1. 1st load: script executes → `globalThis.render` set → namespace JS moves it to `__ssr_bundles[id]` → sets `globalThis.render = undefined`
2. 2nd load: `is_new = false`, skips script eval, runs namespace JS which checks `typeof globalThis.render !== 'function'` → **throws** `"Bundle did not assign a function to globalThis.render"`

**Fix option A** — track `(bundle_path, bundle_id)` pairs:
```rust
let is_new = loaded_paths.insert((bundle_path.to_owned(), bundle_id.to_owned()));
```
Change `loaded_paths` type to `HashSet<(String, String)>` in both the function signature
(`mod.rs:542`) and the caller's initialization (`mod.rs:409`).

**Fix option B** — skip `globalThis.render` check if bundle_id already in `__ssr_bundles`:
The namespace JS already has `if (typeof globalThis.__ssr_bundles[id] !== 'undefined') { return; }`,
so on reload of same id it's already idempotent. The failure only happens on first registration
of a *new* id for an *already-seen* path. The namespace JS could instead re-execute the script
for that case, or the check order could be rearranged.

---

## Bug 2 — `to_js_string` fallback is JS-injection vector

**File:** `src/deno_runtime_wrapper/render.rs:28-30`, `mod.rs:564-565`  
**Severity:** Medium (unreachable in practice, but latent)

```rust
serde_json::to_string(s).unwrap_or_else(|_| format!("\"{}\"", s))
```

Fallback does zero escaping — a `bundle_id` or `args_json` containing `"` or `\` or JS code
would produce script injection. `serde_json::to_string(&str)` on valid UTF-8 never fails,
so this path is unreachable. But the fallback communicates a false assumption.

**Fix:**
```rust
serde_json::to_string(s).expect("serde_json::to_string cannot fail for &str")
```

Same pattern appears in `load_bundle_in_worker`'s `bundle_id_js` computation.

---

## Bug 3 — Chunked render globals not cleaned up on `begin_render` failure

**File:** `src/deno_runtime_wrapper/render_chunked.rs:77-79`  
**Severity:** Low (not a functional bug — overwritten on next render)

The startup script sets `__ssr_chunks = []` and `__ssr_push_chunk` before the
bundle-not-found guard. If `begin_render` fails (e.g., BundleNotFound), `?` propagates
and the cleanup block at line 119-123 is skipped. Globals remain set until next render.

**Fix:** Clean up all globals set by the startup script before the bundle guard:
```rust
let (watchdog, timeout_triggered) = begin_render(...).map_err(|e| {
    let _ = worker.execute_script(
        "<ssr-deno:render-chunked-cleanup>",
        "globalThis.__ssr_deno_result = undefined; \
         globalThis.__ssr_deno_error = undefined; \
         globalThis.__ssr_chunks = undefined; \
         globalThis.__ssr_push_chunk = undefined;"
            .to_string()
            .into(),
    );
    e
})?;
```
Normal success path (`end_render` + line 118-123) already resets these on completion.

---

## Optimization 1 — `intern_script_name` allocates map key twice on miss

**File:** `src/deno_runtime_wrapper/mod.rs:61-70`

```rust
let leaked: &'static str = Box::leak(name.to_owned().into_boxed_str());
guard.insert(name.to_owned(), leaked);  // second allocation
```

`leaked` is `&'static str` which implements `Borrow<str>`, so it can serve as its own map key.
Change the map type to `HashMap<&'static str, &'static str>` and reuse the leaked pointer:

```rust
static SCRIPT_NAMES: OnceLock<Mutex<HashMap<&'static str, &'static str>>> = OnceLock::new();

fn intern_script_name(name: &str) -> &'static str {
    let map = SCRIPT_NAMES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = map.lock().unwrap();
    if let Some(&cached) = guard.get(name) {
        return cached;
    }
    let leaked: &'static str = Box::leak(name.to_owned().into_boxed_str());
    guard.insert(leaked, leaked);
    leaked
}
```

Saves one `String` allocation per miss. `guard.get(name)` still works because
`HashMap<&'static str, _>` can be queried with `&str` via the `Borrow` blanket impl.

---

## Optimization 2 — Watchdog spawns OS thread per render

**File:** `src/deno_runtime_wrapper/watchdog.rs:36-51`

Every `begin_render` call spawns and joins an OS thread. For typical SSR latency (>10ms)
this is acceptable, but at high concurrency (large pool × many requests/sec) the
spawn/join overhead accumulates.

**Future option:** one long-lived watchdog actor per isolate. The actor receives
`(deadline, cancel_token)` messages and manages timers internally, eliminating per-render
thread creation. Deferred — not urgent given current SSR latency profile.

---

## Observation — Partial broadcast leaves pool in inconsistent bundle state

**File:** `src/deno_runtime_wrapper/mod.rs:336-355`

`load_bundle` broadcasts to all isolates. If a worker dies mid-broadcast (`blocking_send`
returns `Err`), isolates 0..N-1 got the bundle and the dead isolate didn't. Round-robin
will dispatch to the partially-loaded isolates (success), but the dead worker is
permanently excluded. No isolate replacement mechanism exists.

Not fixable without a restart/health-check strategy. Add `// TODO: replace dead isolate` comment at the broadcast error site and file a future plan for dead-isolate replacement.

---

## Minor — `_specifier` used despite underscore prefix

**File:** `src/nop_types.rs:68`

```rust
fn resolve_package_folder_from_package(&self, _specifier: &str, ...) {
    ...package_name: _specifier.to_string(),
```

`_specifier` signals "unused" by convention but the value is read. Rename to `specifier`.

---

## Status

| # | Item | Priority | Done |
|---|------|----------|------|
| 1 | Same-path/different-bundle-id bug | High | [x] |
| 2 | `to_js_string` unsafe fallback | Medium | [x] |
| 3 | Chunked globals cleanup on early error | Low | [x] |
| 4 | `intern_script_name` double allocation | Low | [ ] |
| 5 | `_specifier` rename | Trivial | [x] |
| 6 | Watchdog per-render thread (future) | Future | [ ] |
| 7 | Dead isolate replacement (future) | Future | [ ] |
