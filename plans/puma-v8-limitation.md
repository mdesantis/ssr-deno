# Plan: Puma Clustered Mode — V8 TLS Limitation

## Context

The original approach (see [archived/puma-reset.md](archived/puma-reset.md)) was:
`SSR::Deno.reset!` drops the pool, pool re-initializes lazily on first render
in each forked worker. Generation counter + `ensure_loaded` reloads bundles.

**This does not work.** V8 uses a per-process/per-thread slot
(`g_per_isolate_slot_initialized_`) that is set once during the first V8
isolate creation. After `fork`, the child inherits this state. Calling
`reset!` and then creating new V8 isolates in the child hits:

```
Check failed: !g_per_isolate_slot_initialized_
```

V8 isolates **cannot** be created after fork, regardless of how cleanly the
Rust-level pool state is reset. The limitation is in V8's internals — it is
a one-shot-per-process initialization.

## Correct strategy

**Defer `Bundle.new` to `on_worker_boot`.** Never call `Bundle.new` before
fork. The pool initializes lazily on first render, so as long as no Bundle
is created in the master process, each worker creates its own pool on first
use.

```ruby
# config/puma.rb
# ❌ NO: preload_app! + Bundle.new in app initializers
# ✅ YES:
on_worker_boot do
  SSR::Deno::Bundle.new("path/to/bundle.js")
end
```

Users who must use `preload_app!` should configure SSR::Deno setters
(`isolate_pool_size=`, `max_heap_size_mb=`, etc.) before fork but defer
the first `Bundle.new` to `on_worker_boot`.

## Tasks

- [x] Write failing test proving V8 can't create isolates after fork
- [x] Remove `native_reset_pool`, `native_pool_generation`, `reset!`,
      `ensure_loaded`, `@pool_generation`, `Drop for IsolatePool`
- [x] Revert `POOL` from `RwLock<Option<Arc>>` back to `OnceLock`,
      `INITIALIZED` from `AtomicBool` back to `OnceLock<()>`
- [x] Remove `test/ssr/test_deno_reset.rb`
- [x] Remove Puma clustered mode section from `docs/compatibility.md`
- [x] Remove CHANGELOG entry
- [x] Archive old plan, cross-reference

## Cross-reference

See [archived/puma-reset.md](archived/puma-reset.md) for the original
(failed) approach. This plan supersedes it.
(failed) approach. This plan supersedes it.

## Verification

- `bundle exec rake` — full pipeline exits 0
- Tests pass without any `reset!` or fork-after-init code
