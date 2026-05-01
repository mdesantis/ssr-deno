# Configurable Render Timeout

> **Source:** extension of [`render-timeout.md`](render-timeout.md) — the timeout is currently a hardcoded `const RENDER_TIMEOUT: Duration = Duration::from_secs(10)` in `deno_runtime_wrapper.rs`.

---

## Problem

The 10s render timeout is hardcoded. Production SSR may need longer (complex pages), and tests need shorter (fast feedback). A Ruby API lets callers tune it per deployment without touching Rust code.

## Approach

Mirror the existing `max_heap_size_mb` / `isolate_pool_size` pattern:
1. Add `render_timeout_ms: u64` to `Config` in `ssr_deno_core` (default 500)
2. Add a setter (`native_set_render_timeout_ms`) that writes to `CONFIG` before pool init
3. Plumb `render_timeout_ms` through `IsolatePool::new` → `IsolateHandle::spawn` → stored in `IsolateHandle`
4. `block_on_render` reads `self.render_timeout_ms` instead of the static `const`
5. Remove the now-unused `const RENDER_TIMEOUT`
6. Expose via Ruby: `SSR::Deno.render_timeout_ms = 500`
7. Refactor timeout tests to use 200ms timeout + 500ms JS spin — cuts pipeline from 35s to ~5s

### Validation

- **Min**: 100ms — guards against accidental zero/microsecond values
- **Max**: 300000ms (5 min) — sanity ceiling
- **Must be set before pool init** — same guard as `max_heap_size_mb` / `isolate_pool_size`
- **Validation lives in `ssr_deno_core`** — keeps the zero-dep crate testable

### Why per-handle, not per-pool

`IsolatePool` already copies `max_heap_size_mb` into each handle during spawn. Storing `render_timeout_ms` the same way keeps `block_on_render` self-contained (no cross-reference to pool), and is consistent with the existing pattern.

---

## Changes

### 1. `ssr_deno_core/src/lib.rs` — Config + validation

Add field to `Config`:

```rust
pub struct Config {
    pub max_heap_size_mb: usize,
    pub isolate_pool_size: usize,
    pub render_timeout_ms: u64,
}

impl Config {
    pub const fn default() -> Self {
        Self {
            max_heap_size_mb: 64,
            isolate_pool_size: 0,
            render_timeout_ms: 500,
        }
    }
}
```

Add validation:

```rust
/// Validates that `ms` is within [100, 300000].
pub fn validate_render_timeout_ms(ms: u64) -> Result<(), String> {
    if ms < 100 {
        Err("Render timeout must be at least 100ms".into())
    } else if ms > 300_000 {
        Err("Render timeout must not exceed 300000ms (5min)".into())
    } else {
        Ok(())
    }
}
```

Add unit test:

```rust
#[test]
fn config_default_render_timeout() {
    let cfg = Config::default();
    assert_eq!(cfg.render_timeout_ms, 500);
}

#[test]
fn validate_render_timeout_accepts_100() {
    assert!(validate_render_timeout_ms(100).is_ok());
}

#[test]
fn validate_render_timeout_rejects_99() {
    assert!(validate_render_timeout_ms(99).is_err());
}

#[test]
fn validate_render_timeout_accepts_300000() {
    assert!(validate_render_timeout_ms(300_000).is_ok());
}

#[test]
fn validate_render_timeout_rejects_300001() {
    assert!(validate_render_timeout_ms(300_001).is_err());
}
```

### 2. `ext/ssr_deno/src/deno_runtime_wrapper.rs` — plumbing

**Remove `const RENDER_TIMEOUT`.**

**`IsolateHandle`** — add `render_timeout_ms` field:

```rust
pub struct IsolateHandle {
    tx: tokio::sync::mpsc::Sender<WorkerMsg>,
    render_timeout_ms: u64,
}
```

**`IsolateHandle::spawn`** — accept timeout parameter:

```rust
pub fn spawn(index: usize, max_heap_size_mb: usize, render_timeout_ms: u64) -> Result<Self, DenoError> {
    // ... existing spawn logic ...
    Ok(Self { tx, render_timeout_ms })
}
```

**`IsolatePool::new`** — accept and forward timeout:

```rust
pub fn new(size: usize, max_heap_size_mb: usize, render_timeout_ms: u64) -> Result<Self, DenoError> {
    validate_pool_size(size)?;
    let mut handles = Vec::with_capacity(size);
    for i in 0..size {
        let handle = IsolateHandle::spawn(i, max_heap_size_mb, render_timeout_ms)?;
        handles.push(handle);
    }
    Ok(Self { handles, counter: AtomicUsize::new(0) })
}
```

**`block_on_render`** — use `self.render_timeout_ms`:

```rust
pub fn block_on_render(&self, bundle_id: &str, args_json: &str) -> Result<String, DenoError> {
    let timeout = Duration::from_millis(self.render_timeout_ms);
    // ... same send logic ...
    match reply_rx.recv_timeout(timeout) {
        Ok(result) => result,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            Err(DenoError::Render(
                format!("Render timed out after {}ms", timeout.as_millis()),
            ))
        }
        // ... disconnected arm unchanged ...
    }
}
```

### 3. `ext/ssr_deno/src/lib.rs` — native setter

Mirror `native_set_max_heap_size_mb` pattern:

```rust
fn native_set_render_timeout_ms(ms: u64) -> Result<(), Error> {
    if let Err(msg) = ssr_deno_core::validate_render_timeout_ms(ms) {
        return Err(Error::new(
            Ruby::get().unwrap().exception_arg_error(),
            msg,
        ));
    }
    check_not_initialized()?;
    CONFIG.lock().unwrap().render_timeout_ms = ms;
    Ok(())
}
```

Register in `init_ssr_deno`:

```rust
deno_module.define_module_function("native_set_render_timeout_ms", function!(native_set_render_timeout_ms, 1))?;
```

Pass config to pool:

```rust
// In get_or_init_pool:
let render_timeout_ms = config.render_timeout_ms;
let pool = IsolatePool::new(pool_size, max_heap_size_mb, render_timeout_ms)
    .map_err(|e| js_runtime_initialization_error(e.to_string()))?;
```

### 4. `lib/ssr/deno.rb` — Ruby accessor

```ruby
# @param ms [Integer] render timeout in milliseconds (min 100, max 300000)
# @raise [JsRuntimeInitializationError] if pool already initialized
# @raise [ArgumentError] if ms is out of valid range
def self.render_timeout_ms=(ms)
  native_set_render_timeout_ms(ms.to_i)
end
```

### 5. Tests — refactor for speed

With `render_timeout_ms = 200`, the JS spin can be 500ms:

```ruby
HANG_JS = <<~JS.chomp
  globalThis.render = function() {
    let end = Date.now() + 500;
    while (Date.now() < end) {}
    return 'timeout did not fire';
  };
JS

def test_render_timeout
  script = <<~RUBY
    ...
    SSR::Deno.render_timeout_ms = 200
    ...
  RUBY
end

def test_render_works_after_timeout
  script = <<~RUBY
    ...
    SSR::Deno.render_timeout_ms = 200
    SSR::Deno.isolate_pool_size = 2
    ...
  RUBY
end
```

**Result**: each test takes ~200ms instead of ~10s → pipeline drops from 35s to ~5s.

### 6. Docs updates

- `sig/ssr/deno.rbs` — add `self.render_timeout_ms=: (Integer ms) -> nil`
- `CHANGELOG.md` — add `render_timeout_ms=` entry
- `README.md` — add config example alongside `max_heap_size_mb`

---

## Implementation Order

1. [ ] Add `render_timeout_ms` to `Config` + validation + unit tests in `ssr_deno_core`
2. [ ] Plumb `render_timeout_ms` through `IsolatePool::new` → `IsolateHandle::spawn` → `block_on_render`
3. [ ] Remove `const RENDER_TIMEOUT`
4. [ ] Add `native_set_render_timeout_ms` in `lib.rs`
5. [ ] Add Ruby accessor in `lib/ssr/deno.rb`
6. [ ] Refactor timeout tests: 200ms timeout, 500ms JS spin
7. [ ] Update `sig/ssr/deno.rbs`
8. [ ] Update `CHANGELOG.md`
9. [ ] Run `bundle exec rake` to verify full pipeline
