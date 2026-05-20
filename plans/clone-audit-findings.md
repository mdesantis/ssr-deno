# Clone Audit — `.clone()` → Lifetime Opportunities

Audit date: 2026-05-20. Coverage: all `.rs` files in `ext/ssr_deno/src/` and `crates/`.

---

## 🔴 Actionable

### 1. `set_aliases` — owned HashMap drained instead of cloned

**File:** `ext/ssr_deno/crates/ssr_deno_dev_mode/src/dev_mode_module_loader.rs:788`

**Current:**
```rust
pub fn set_aliases(shared: &SharedAliasMap, aliases: &HashMap<String, String>) {
    let mut sorted: Vec<(String, String)> = aliases
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
```

**Why:** The call chain starts with an owned `HashMap<String, String>` in `native_dev_load_entry` (lib.rs). It's passed by ref through 3 functions before reaching `set_aliases`. Passing ownership all the way allows using `.into_iter()` — zero allocations.

**Fix chain:**
- `dev_mode_module_loader.rs`: `set_aliases` takes `HashMap<String, String>` by value, uses `into_iter()`
- `engine/dev_load.rs`: `dev_load_entry` takes `HashMap<String, String>` by value, passes by move
- `engine/dev_worker.rs`: `dev_worker_thread_main` passes `resolve_alias` by move instead of `&`

**Impact:** N×2 allocations saved per `dev_load_entry` call (N = alias count, typically 5–20). Higher impact with frequent auto-reload.

**Risk:** Low — mechanical ownership change, no logic change.

---

### 2. `build_dev_node_services` — take parts by value, return unused fields

**File:** `ext/ssr_deno/crates/ssr_deno_dev_mode/src/dev_mode_builder.rs:36-40`

**Current:**
```rust
fn build_dev_node_services(parts: &DevModeNpmResolverParts) -> Option<DevNodeServices> {
    let r = NodeResolver::new(
        parts.npm_checker.clone(),      // ZST, free
        DenoIsBuiltInNodeModuleChecker,
        parts.npm_resolver.clone(),     // clone — could move
        parts.pkg_json_resolver.clone(), // Rc clone — cheap, needed
        parts.node_resolution_sys.clone(), // clone — could move
        ...
    );
```

**Why:** `build_dev_mode_worker` borrows `resolver_parts` (line 65) to call `build_dev_node_services`, then moves `resolver_parts` into `DevModeModuleLoader::new` (line 73). The borrow forces cloning `npm_resolver` and `node_resolution_sys`. Taking by value and returning unused components eliminates those clones.

**Fix:** Change signature to `fn build_dev_node_services(parts: DevModeNpmResolverParts) -> (Option<DevNodeServices>, PackageJsonResolverRc<Sys>)`. Caller destructures and passes `pkg_json_resolver` to `DevModeModuleLoader::new`. Must handle the `Option::None` case (currently unreachable, but the signature allows it).

**Impact:** Two non-trivial clones eliminated per worker init. Only matters during cold start / reload.

**Risk:** Low — mechanical change, touches one function.

---

## 🟡 Trivial

### 3. `cjs_shim` — PathBuf clone on last use

**File:** `ext/ssr_deno/crates/ssr_deno_dev_mode/src/dev_mode_module_loader.rs:739`

**Current:**
```rust
guard.push(canonical.clone());
```

**Why:** `canonical` is a `PathBuf`. It was borrowed once at line 733 (`analyze_cjs_exports(&canonical)`) — a borrow, not a move, so ownership is still available. Line 739 is the last use of `canonical`; nothing references it after the push. Move eliminates one PathBuf allocation.

**Fix:**
```rust
guard.push(canonical);
```

**Impact:** One PathBuf allocation eliminated per unique CJS file per worker lifetime. Called once per `load_main_es_module` on a CJS-detected path.

**Risk:** None — trivial last-use move.

---

### 4. `intern_script_name` — double allocation on cache miss

**File:** `ext/ssr_deno/src/engine/mod.rs:55-56`

**Current:**
```rust
fn intern_script_name(name: &str) -> &'static str {
    ...
    let leaked = Box::leak(name.to_owned().into_boxed_str());  // alloc #1
    guard.insert(name.to_owned(), leaked);                     // alloc #2
```

**Fix:**
```rust
    let owned = name.to_owned();
    let leaked = Box::leak(owned.clone().into_boxed_str());
    guard.insert(owned, leaked);
```

**Impact:** One `String` allocation eliminated on cache miss (rare — only for new script names).

---

## ✅ Not actionable (correct by design)

| Pattern | Reason |
|---------|--------|
| `Arc`/`Rc` clones | Cheap refcount bump |
| Channel-boundary `.to_string()` | Must own data for `Send` |
| Lock-boundary PathBuf/String clones | Must release mutex before I/O |
| Magnus FFI `String` params | Dictated by `function!` macro |
| `.to_string()` for JS script literals | Must construct script string |
| `Config` passed by value | `Copy` type, zero-cost |
