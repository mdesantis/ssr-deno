# Security Review — ext/ssr_deno

Date: 2026-04-28
Scope: `ext/ssr_deno/src/` (recent commits on `main`)

---

## ~~CRITICAL~~ ✅ FIXED

### ~~`PermissionsContainer::allow_all`~~ — `deno_runtime_wrapper.rs:168`

**Fixed.** Worker now runs with `Permissions::none_without_prompt()` — all Deno
permissions (net, fs, env, run, ffi) denied. `AllowAllPermissionDescriptorParser`
renamed to `NopPermissionDescriptorParser` to reflect its actual role.

```rust
permissions: PermissionsContainer::new(
    Arc::new(NopPermissionDescriptorParser),
    Permissions::none_without_prompt(),
),
```

---

## HIGH

### `RealFs` — `deno_runtime_wrapper.rs:164` — ACCEPTED RISK

`RealFs` remains in place. With deny-all permissions + `NoopModuleLoader`, it is
unreachable from JS. Only a Deno CVE bypassing the permissions layer would expose it.
`FileSystem` trait has ~115 abstract methods — a `NopFs` impl would be substantial
maintenance overhead for marginal gain. Accepted as residual risk.

### ~~`FsModuleLoader`~~ — `deno_runtime_wrapper.rs:165` ✅ FIXED

**Fixed.** Replaced with `NoopModuleLoader` — all dynamic `import()` calls now
rejected at the loader level, independent of permissions.

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

### ~~TOCTOU in `init_runtime`~~ — `lib.rs:34-43` ✅ FIXED

**Fixed** via double-checked locking with a static `INIT_LOCK: Mutex<()>`:

```rust
static INIT_LOCK: Mutex<()> = Mutex::new(());

fn init_runtime(bundle_path: String) -> Result<Option<bool>, Error> {
    if RUNTIME.get().is_some() { return Ok(None); }   // fast path (no lock)
    let _guard = INIT_LOCK.lock().unwrap();
    if RUNTIME.get().is_some() { return Ok(None); }   // re-check under lock
    let runtime = DenoRuntimeWrapper::new(&bundle_path)
        .map_err(|e| runtime_error(format!("Failed to initialize runtime: {e}")))?;
    let _ = RUNTIME.set(runtime);
    Ok(Some(true))
}
```

`DenoRuntimeWrapper::new()` now runs exactly once. `Box::leak` bounded to 1.

---

## LOW

### `Box::leak` per init — `deno_runtime_wrapper.rs:56-60`

One `Box::leak` per `DenoRuntimeWrapper::new()` call. Now bounded to exactly 1 (TOCTOU
fixed). Acceptable for a process-lifetime singleton.

### Filesystem paths in error messages — `deno_runtime_wrapper.rs:50,52,127`

Full canonical paths appear in error strings that propagate up to Ruby exceptions.
Leaks server filesystem structure in error responses.

**Fix:** Strip or sanitize paths from user-facing error messages. Log full paths
internally, return generic messages externally.

---

## Summary

| Severity | File | Line | Issue |
|----------|------|------|-------|
| ~~Critical~~ | `deno_runtime_wrapper.rs` | 168 | ~~`allow_all` permissions~~ ✅ |
| High | `deno_runtime_wrapper.rs` | 164 | `RealFs` — accepted risk (mitigated by deny-all perms + NoopModuleLoader) |
| ~~High~~ | `deno_runtime_wrapper.rs` | 165 | ~~`FsModuleLoader` — dynamic imports from fs~~ ✅ |
| Medium | `deno_runtime_wrapper.rs` | 49–52 | No bundle path boundary validation |
| ~~Medium~~ | `lib.rs` | 34–43 | ~~TOCTOU between `is_some()` check and `set()`~~ ✅ |
| Low | `deno_runtime_wrapper.rs` | 56–60 | `Box::leak` per init (bounded to 1) |
| Low | `deno_runtime_wrapper.rs` | 50,52,127 | Full paths in error messages |

**Priority:** Fix `allow_all` first — it is the root of the Critical + both High findings.
