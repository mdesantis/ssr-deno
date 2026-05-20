# Clone Audit — `.clone()` → Lifetime Opportunities

Audit date: 2026-05-20. Coverage: all `.rs` files in `ext/ssr_deno/src/` and `crates/`.

---

## 🔴 Actionable

### 1. `set_aliases` — owned HashMap drained instead of cloned ✅ DONE

**File:** `ext/ssr_deno/crates/ssr_deno_dev_mode/src/dev_mode_module_loader.rs:788`

**Before:**
```rust
pub fn set_aliases(shared: &SharedAliasMap, aliases: &HashMap<String, String>) {
    let mut sorted: Vec<(String, String)> = aliases
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
```

**After:**
```rust
pub fn set_aliases(shared: &SharedAliasMap, aliases: HashMap<String, String>) {
    let mut sorted: Vec<(String, String)> = aliases.into_iter().collect();
```

**Chain:** `set_aliases` takes owned `HashMap`, uses `into_iter()`. `dev_load_entry` takes `HashMap<String, String>` by value. `dev_worker_thread_main` passes `resolve_alias` by move.

---

### 2. `build_dev_node_services` — ✅ NOT ACTIONABLE (architectural constraint)

**File:** `ext/ssr_deno/crates/ssr_deno_dev_mode/src/dev_mode_builder.rs:36-40`

**Current:**
```rust
fn build_dev_node_services(parts: &DevModeNpmResolverParts) -> Option<DevNodeServices> {
    let r = NodeResolver::new(
        parts.npm_checker.clone(),      // ZST, free
        ...
        parts.npm_resolver.clone(),     // consumed by NodeResolver::new
        parts.pkg_json_resolver.clone(), // Rc clone — cheap
        parts.node_resolution_sys.clone(),
        ...
    );
    ...
    Some(DevNodeServices { node_resolver: MaybeArc::new(r), ... })
}
```

**Why not actionable:** Both `build_dev_node_services` and `DevModeModuleLoader::new` construct separate `NodeResolver` instances, each **consuming** their own `npm_resolver` and `node_resolution_sys`. Taking `parts` by value would move these into `build_dev_node_services`, leaving nothing for `DevModeModuleLoader::new`. The two clones are inherent to the dual-`NodeResolver` architecture.

The `DevModeNpmResolverParts` refactor already eliminated the **redundant** `build_dev_mode_npm_resolver` call — that was the big win. The remaining clones are the cost of two independent resolver instances.

**Real fix (deeper):** Refactor so `build_dev_node_services` and `DevModeModuleLoader` share the same `NodeResolver` instance. `build_dev_node_services` wraps it in `MaybeArc`, while `DevModeModuleLoader` stores the raw type — these would need to be unified. Not worth the churn for a one-per-worker-init overhead.

---

## 🟡 Trivial

### 3. `cjs_shim` — PathBuf clone on last use ✅ DONE

**File:** `ext/ssr_deno/crates/ssr_deno_dev_mode/src/dev_mode_module_loader.rs:739`

**Before:**
```rust
guard.push(canonical.clone());
```

**After:**
```rust
guard.push(canonical);
```

**Why:** `canonical` was last-clone-before-move. `analyze_cjs_exports(&canonical)` (line 733) is a borrow, not a move — ownership still held for the push on line 739.

---

### 4. `intern_script_name` — double allocation on cache miss ✅ DONE

**File:** `ext/ssr_deno/src/engine/mod.rs:55-56`

**Before:**
```rust
let leaked: &'static str = Box::leak(name.to_owned().into_boxed_str());  // alloc #1
guard.insert(name.to_owned(), leaked);                                     // alloc #2
```

**After:**
```rust
let owned = name.to_owned();
let leaked: &'static str = Box::leak(owned.clone().into_boxed_str());
guard.insert(owned, leaked);
```

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
