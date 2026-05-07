# Plan: `SSR::Deno.reset!` — Puma Clustered Mode Compatibility

**SUPERSEDED by [puma-v8-limitation.md](puma-v8-limitation.md).** V8 cannot
create isolates after fork (`g_per_isolate_slot_initialized_` assertion).
The `reset!` + re-create approach is impossible. Correct strategy: defer
`Bundle.new` to `on_worker_boot`.

## Context

In Puma clustered mode with `preload_app!`, the master process loads the app (and initializes the SSR pool via `Bundle.new`) before forking workers. After fork, the child inherits a corrupted pool: V8 isolates and per-isolate tokio runtimes reference threads that only exist in the parent. This is classic "fork-after-thread" UB.

Concretely: after fork, the child's `tokio::sync::mpsc::Sender` can still enqueue a message (capacity 1 — first send succeeds), but `blocking_recv()` on the reply channel blocks forever — the worker thread that would process the message and send the reply does not exist in the child. The V8 watchdog thread also doesn't exist, so render_timeout_ms cannot fire. **Result: deadlock.**

`SSR::Deno.reset!` drops the inherited pool, resets the INITIALIZED flag, increments a generation counter, and lets the pool re-initialize lazily on the first render in each worker. Existing `Bundle` objects detect the generation mismatch and reload transparently.

Correct usage in `puma.rb`:
```ruby
on_worker_boot do
  SSR::Deno.reset!
  # Config from parent is preserved; bundles reload lazily on first render
end
```

---

## Implementation order (TDD)

### ✅ Step 1 — Write failing test: `test/ssr/test_deno_reset.rb`

New test file, class `TestDenoReset < Minitest::Test`, includes `SubprocessHelper`.

All tests use subprocess (via `run_subprocess`) to isolate from the global test pool.

**Test A — prove the problem (passes before and after implementation, documents the hang):**
```ruby
def test_render_deadlocks_in_forked_child_without_reset
  bundle_path = TestFixturePaths::MINIMAL_BUNDLE

  script = "bundle_path = '#{bundle_path}'\n"
  script << <<~'RUBY'
    bundle = SSR::Deno::Bundle.new(bundle_path)

    r, w = IO.pipe
    pid = Process.fork do
      r.close
      begin
        bundle.render(nil)
        w.write('unexpected'); w.close; exit!(0)
      rescue => e
        w.write("err:#{e.class}"); w.close; exit!(1)
      end
    end
    w.close

    readable = IO.select([r], nil, nil, 3)
    if readable.nil?
      Process.kill(:KILL, pid)
      Process.waitpid(pid)
      exit 0
    else
      output = r.read
      _, status = Process.waitpid2(pid)
      exit 1
    end
  RUBY

  assert_subprocess(script, 'Expected forked child to deadlock without reset!')
end
```
Child hangs in native `blocking_recv` — GIL not released, so Ruby timer threads can't run. Use `IO.select` with 3s timeout in parent instead. Timeout → SIGKILL child → exit 0 proves deadlock.

**Test B — feature test (FAILS before implementation):**
```ruby
def test_render_succeeds_in_forked_child_after_reset
  bundle_path = TestFixturePaths::MINIMAL_BUNDLE

  script = "bundle_path = '#{bundle_path}'\n"
  script << <<~'RUBY'
    bundle = SSR::Deno::Bundle.new(bundle_path)

    r, w = IO.pipe
    pid = Process.fork do
      r.close
      SSR::Deno.reset!
      result = bundle.render(nil)
      w.write(result.nil? ? 'nil' : 'ok')
      w.close
      exit!(0)
    end
    w.close
    output = r.read; r.close
    Process.waitpid(pid)
    raise "Render failed in forked child: #{output.inspect}" unless output == 'ok'
    exit 0
  RUBY

  assert_subprocess(script, 'Expected render to succeed in forked worker after SSR::Deno.reset!')
end
```

**Test C — config setters available again after reset:**
```ruby
def test_config_setters_available_after_reset
  bundle_path = TestFixturePaths::MINIMAL_BUNDLE

  script = "bundle_path = '#{bundle_path}'\n"
  script << <<~RUBY
    SSR::Deno::Bundle.new(bundle_path)
    SSR::Deno.reset!
    SSR::Deno.isolate_pool_size = 2
    exit 0
  RUBY

  assert_subprocess(script, 'Expected config setters to work after reset!')
end
```

**Test D — pool generation increments:**
```ruby
def test_pool_generation_increments_on_reset
  bundle_path = TestFixturePaths::MINIMAL_BUNDLE

  script = "bundle_path = '#{bundle_path}'\n"
  script << <<~RUBY
    SSR::Deno::Bundle.new(bundle_path)
    gen_before = SSR::Deno.native_pool_generation
    SSR::Deno.reset!
    gen_after = SSR::Deno.native_pool_generation
    raise "Expected generation to increment" unless gen_after == gen_before + 1
    exit 0
  RUBY

  assert_subprocess(script, 'Expected pool generation to increment after reset!')
end
```

Notes: `<<~'RUBY'` (single-quoted) prevents inner `#{e.class}` etc from being interpolated in outer scope — needed because subprocess scripts share the test file's `#{}` syntax. `<<~RUBY` (unquoted) OK when heredoc contains no interpolation. `bundle_path` interpolated via `"bundle_path = '#{bundle_path}'\n"` prefix string.

Run `bundle exec rake test` → **Test B, C, D fail** (`NoMethodError: undefined method 'reset!'`). Test A passes. Then implement.

---

### ✅ Step 2 — Rust: `ext/ssr_deno/src/lib.rs`

**Replace global state:**
```rust
// Remove:
static POOL: OnceLock<IsolatePool> = OnceLock::new();
static POOL_INIT_LOCK: Mutex<()> = Mutex::new(());
static INITIALIZED: OnceLock<()> = OnceLock::new();

// Add:
static POOL: RwLock<Option<Arc<IsolatePool>>> = RwLock::new(None);
static INITIALIZED: AtomicBool = AtomicBool::new(false);
static POOL_GENERATION: AtomicU64 = AtomicU64::new(0);
```

`POOL_GENERATION` is an `AtomicU64`. Bundle instances stamp a local `@pool_generation` after each `load`. Generation mismatch → `ensure_loaded` re-runs `load`. Fully thread-safe: reads are `Ordering::SeqCst`, write is `fetch_add` in `native_reset_pool`.

Imports: add `Arc`, `RwLock`, `AtomicBool`, `AtomicU64`, `Ordering`; remove `OnceLock`.

**Rewrite `check_not_initialized`:**
```rust
fn check_not_initialized() -> Result<(), Error> {
    if INITIALIZED.load(Ordering::SeqCst) {
        Err(Error::new(deno_exception_class("JsRuntimeInitializationError"),
            "Cannot set config after runtime is already initialized"))
    } else {
        Ok(())
    }
}
```

**Rewrite `get_or_init_pool`** (returns `Arc<IsolatePool>`; `POOL`'s write lock replaces `POOL_INIT_LOCK`):
```rust
fn get_or_init_pool() -> Result<Arc<IsolatePool>, Error> {
    {
        let guard = POOL.read().unwrap();
        if let Some(pool) = guard.as_ref() {
            return Ok(Arc::clone(pool));
        }
    }
    let mut guard = POOL.write().unwrap();
    if let Some(pool) = guard.as_ref() {
        return Ok(Arc::clone(pool));
    }

    let config = *CONFIG.lock().unwrap();
    let pool_size = ssr_deno_core::resolve_pool_size(config);
    let max_heap_size_mb = config.max_heap_size_mb;
    let render_timeout_ms = config.render_timeout_ms;
    let node_builtins = config.node_builtins;

    let pool = Arc::new(
        IsolatePool::new(
            pool_size,
            max_heap_size_mb,
            render_timeout_ms,
            node_builtins,
        )
        .map_err(|e| js_runtime_initialization_error(e.to_string()))?,
    );
    *guard = Some(Arc::clone(&pool));
    INITIALIZED.store(true, Ordering::SeqCst);
    Ok(pool)
}
```

**Rewrite `get_pool`:**
```rust
fn get_pool() -> Result<Arc<IsolatePool>, Error> {
    POOL.read()
        .unwrap()
        .as_ref()
        .map(Arc::clone)
        .ok_or_else(|| {
            js_runtime_not_initialized_error(
                "Runtime not initialized. Call `SSR::Deno::Bundle.new` first.",
            )
        })
}
```

All callers (`native_render`, `native_render_chunks`, `native_heap_stats`) use the returned `Arc` directly — `.method()` still works.

Ractor safety comment updated: `RwLock + AtomicBool` replaces `OnceLock` reference.

**Add `native_reset_pool` and `native_pool_generation`:**
```rust
fn native_reset_pool() -> Result<(), Error> {
    let mut guard = POOL.write().unwrap();
    *guard = None;  // Drop Arc → if last ref, IsolatePool drops → tx channels drop → workers exit
    INITIALIZED.store(false, Ordering::SeqCst);
    POOL_GENERATION.fetch_add(1, Ordering::SeqCst);
    Ok(())
}

fn native_pool_generation() -> u64 {
    POOL_GENERATION.load(Ordering::SeqCst)
}
```
Note: no `&Value` param — 0-arg `function!` means magnus doesn't pass self.

**Register in magnus init block:**
```rust
deno_module.define_singleton_method("native_reset_pool", function!(native_reset_pool, 0))?;
deno_module.define_singleton_method("native_pool_generation", function!(native_pool_generation, 0))?;
```

---

### ✅ Step 3 — Rust: `ext/ssr_deno/src/deno_runtime_wrapper/mod.rs`

**Add `Drop` for `IsolatePool`:**
```rust
impl Drop for IsolatePool {
    fn drop(&mut self) {
        // Dropping tx senders signals workers: rx.recv() returns None → loop exits.
        self.handles.clear();
    }
}
```

Worker threads loop on `while let Some(msg) = rx.recv().await`. When all senders drop, `recv()` returns `None` → threads exit. In the fork case, worker threads don't exist in the child — dropping is a safe no-op.

---

### ✅ Step 4 — Ruby: `lib/ssr/deno.rb`

`reset!` delegates entirely to Rust. No mutable Ruby-side state:

```ruby
def self.reset!
  native_reset_pool
end
```

`native_pool_generation` and `native_reset_pool` are already exposed on the module by the Rust init block.

---

### ✅ Step 5 — Ruby: `lib/ssr/deno/bundle.rb`

`@pool_generation` is a Bundle instance variable (not shared between threads → no synchronization needed).

**In `initialize`:** add `@pool_generation = -1` before calling `load`. (Generation starts at 0; -1 is a safe sentinel that ensures `ensure_loaded` would trigger — but `initialize` calls `load` directly, so it's just a defensive default.)

**Rewrite `load` (private):** stamp generation after successful native call:
```ruby
def load
  SSR::Deno.native_load_bundle(@bundle_id, @bundle_path)
  @pool_generation = SSR::Deno.native_pool_generation
end
```

**Add `ensure_loaded` (private):**
```ruby
def ensure_loaded
  return if @pool_generation == SSR::Deno.native_pool_generation

  instrument 'bundle_load.ssr_deno', bundle_name: @bundle_id, path: @bundle_path do
    load
  end
end
```

**Update `render`:**
```ruby
def render(data = nil, raw_input: false, raw_output: false)
  ensure_loaded
  reload_if_changed if @auto_reload
  # ... rest unchanged
end
```

**Update `render_chunks`:**
```ruby
def render_chunks(data = nil, raw_input: false, &)
  ensure_loaded
  reload_if_changed if @auto_reload
  # ... rest unchanged
end
```

`ensure_loaded` runs before `reload_if_changed`: if pool was reset AND file changed, we load into the new pool first, then overwrite with latest file content.

---

### ✅ Step 6 — RBS: `sig/ssr/deno.rbs`

Add to `SSR::Deno` module:
```rbs
def self.reset!: () -> void
def self.native_reset_pool: () -> void
def self.native_pool_generation: () -> Integer
```

Add to `SSR::Deno::Bundle`:
```rbs
@pool_generation: Integer
```

Add to Bundle's private section:
```rbs
def ensure_loaded: () -> void
```

---

### ✅ Step 7 — Docs: `docs/compatibility.md`

Add **"Puma clustered mode"** section:
- Why `preload_app!` + pool init before fork = deadlock (fork-after-thread, dead worker channels)
- `reset!` fixes it: drops pool, increments generation, lazy re-init on first render
- `puma.rb` snippet: `on_worker_boot { SSR::Deno.reset! }`
- Note: config (heap size, pool size, timeout, node_builtins) is preserved across reset
- Note: unregistered bundles reload too — any `Bundle` instance detects generation mismatch

---

### ✅ Step 8 — CHANGELOG.md

Add under `## Unreleased`:
```
- `SSR::Deno.reset!` — drops and re-initializes the isolate pool. Required when
  using Puma in clustered mode with `preload_app!`. Call in `on_worker_boot`.
  Existing `Bundle` instances reload automatically on the next render.
  Config (heap size, pool size, timeout, node_builtins) is preserved across reset.
```

---

## Critical files

| File | Change |
|------|--------|
| `test/ssr/test_deno_reset.rb` | New — TDD tests (write first) |
| `ext/ssr_deno/src/lib.rs` | Replace OnceLock globals; add `native_reset_pool`, `native_pool_generation` |
| `ext/ssr_deno/src/deno_runtime_wrapper/mod.rs` | Add `Drop` for `IsolatePool` |
| `lib/ssr/deno.rb` | Add `reset!` |
| `lib/ssr/deno/bundle.rb` | Add `@pool_generation`, `ensure_loaded`; update `load`, `render`, `render_chunks` |
| `sig/ssr/deno.rbs` | Add `reset!`, `native_reset_pool`, `native_pool_generation`, `ensure_loaded`, `@pool_generation` |
| `docs/compatibility.md` | Add Puma clustered mode section |
| `CHANGELOG.md` | Add Unreleased entry |

## Verification

1. ✅ Write `test_deno_reset.rb` → `bundle exec rake test` → Test A passes, B/C/D fail
2. ✅ Implement Rust → `bundle exec rake compile`
3. ✅ Implement Ruby → `bundle exec rake test` → all test assertions pass
4. ❌ `bundle exec rake` → full pipeline exits 0
5. ❌ Coverage: `ensure_loaded` (Bundle) and `reset!` (SSR::Deno) uncovered — need tests that exercise these paths under SimpleCov without subprocess isolation
