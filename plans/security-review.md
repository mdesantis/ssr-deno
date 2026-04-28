# Security Review — ext/ssr_deno

Date: 2026-04-28
Scope: `ext/ssr_deno/src/` (recent commits on `main`)

---

## CRITICAL

### `PermissionsContainer::allow_all` — `deno_runtime_wrapper.rs:168`

The Deno worker runs with unrestricted permissions: network, filesystem read/write,
environment variables, subprocess execution, and FFI. An SSR renderer needs none of
these — it only evaluates a pre-bundled JS file.

A compromised or malicious bundle has full host access.

**Fix:** Replace with deny-all permissions:

```rust
permissions: PermissionsContainer::new_deny_all(),
```

---

## HIGH

### `RealFs` — `deno_runtime_wrapper.rs:164`

Real filesystem access is enabled. Combined with `allow_all`, the bundle can call
`Deno.readFile("/etc/shadow")` or write arbitrary files.

**Fix:** Use a no-op or in-memory filesystem implementation, or lock down with deny-all
permissions (see above).

### `FsModuleLoader` — `deno_runtime_wrapper.rs:165`

Dynamic `import()` from the filesystem is enabled. The bundle (or injected code) can
`import("/attacker/payload.js")` with full permissions.

**Fix:** Replace with a module loader that rejects all imports, since the SSR bundle is
self-contained and should never need dynamic imports:

```rust
module_loader: std::rc::Rc::new(deno_runtime::deno_core::NoopModuleLoader),
```

---

## MEDIUM

### No bundle path boundary check — `deno_runtime_wrapper.rs:49-52`

`canonicalize()` resolves symlinks and normalizes the path but never verifies the
result is inside an expected directory. A Ruby caller can pass an arbitrary path and
load any JS file on the system.

**Fix:** Validate the canonical path is within the expected bundle directory:

```rust
let canonical = std::fs::canonicalize(bundle_path)
    .map_err(|e| format!("Cannot resolve bundle path '{bundle_path}': {e}"))?;

if !canonical.starts_with(&expected_bundle_dir) {
    return Err(format!("Bundle path is outside the allowed directory").into());
}
```

### TOCTOU in `init_runtime` — `lib.rs:34-43`

```rust
if RUNTIME.get().is_some() {    // <- thread A passes here
    return Ok(None);
}
// thread B also passes here
let runtime = DenoRuntimeWrapper::new(&bundle_path)...
let _ = RUNTIME.set(runtime);   // one wins, one silently loses
```

Two concurrent callers both pass the `is_some()` guard, both spawn a worker thread and
evaluate the bundle, and both trigger `Box::leak`. The loser's runtime is silently
dropped. Outcome: double bundle execution + double memory leak.

**Fix:** Remove the precheck. `OnceLock::set` returns `Err` if already set — handle
that instead:

```rust
fn init_runtime(bundle_path: String) -> Result<Option<bool>, Error> {
    if RUNTIME.get().is_some() {
        return Ok(None); // fast path (racy but safe to keep for perf)
    }
    let runtime = DenoRuntimeWrapper::new(&bundle_path)
        .map_err(|e| runtime_error(format!("Failed to initialize runtime: {e}")))?;
    match RUNTIME.set(runtime) {
        Ok(_) => Ok(Some(true)),
        Err(_) => Ok(None), // lost the race, already initialized
    }
}
```

This still has the double-init race but eliminates the silent discard and makes intent
explicit. A proper fix uses a `Mutex<Option<...>>` or `OnceCell` from `once_cell` crate.

---

## LOW

### `Box::leak` per init — `deno_runtime_wrapper.rs:56-60`

One `Box::leak` per `DenoRuntimeWrapper::new()` call. Bounded to 1 with a correct
singleton, but the TOCTOU above allows 2+. Fix the TOCTOU first; the leak itself is
acceptable for a process-lifetime singleton.

### Filesystem paths in error messages — `deno_runtime_wrapper.rs:50,52,127`

Full canonical paths appear in error strings that propagate up to Ruby exceptions.
Leaks server filesystem structure in error responses.

**Fix:** Strip or sanitize paths from user-facing error messages. Log full paths
internally, return generic messages externally.

---

## Summary

| Severity | File | Line | Issue |
|----------|------|------|-------|
| Critical | `deno_runtime_wrapper.rs` | 168 | `allow_all` permissions |
| High | `deno_runtime_wrapper.rs` | 164 | `RealFs` — real filesystem access |
| High | `deno_runtime_wrapper.rs` | 165 | `FsModuleLoader` — dynamic imports from fs |
| Medium | `deno_runtime_wrapper.rs` | 49–52 | No bundle path boundary validation |
| Medium | `lib.rs` | 34–43 | TOCTOU between `is_some()` check and `set()` |
| Low | `deno_runtime_wrapper.rs` | 56–60 | `Box::leak` per init (bounded after TOCTOU fix) |
| Low | `deno_runtime_wrapper.rs` | 50,52,127 | Full paths in error messages |

**Priority:** Fix `allow_all` first — it is the root of the Critical + both High findings.
