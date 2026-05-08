# Ruby Codebase Review — lib/, test/, scripts/

_Reviewed: 2026-05-08_

---

## Bug 1 — `ssr_render` calls `.html_safe` on bundle results

**File:** `lib/ssr/deno/rails/helper.rb:26`  
**Priority:** High  
**Status:** Fixed ✅

**.html_safe is banned from the codebase.** The helper must not mark any string
as `html_safe`. Raw strings are returned as-is from `bundle.render`. The caller
(app view) is responsible for marking output safe when needed.

```ruby
# Before (original):
bundle.render(data, **options).html_safe

# Before (previous fix):
result = bundle.render(data, **options)
result.is_a?(String) ? result.html_safe : result

# After (final — no .html_safe anywhere):
bundle.render(data, **options)
```

Updated documentation removes all mentions of `html_safe`. Empty string CSR
fallback is plain `''`.

---

## Bug 2 — `render_timeout_ms` not wired into Rails config

**File:** `lib/ssr/deno/rails/railtie.rb:22-46`  
**Severity:** Medium

The `init_bundles` initializer applies `max_heap_size_mb`, `isolate_pool_size`, and
`node_builtins_enabled` from `config.ssr_deno`, but never `render_timeout_ms`:

```ruby
# Applied:
SSR::Deno.max_heap_size_mb    = config.ssr_deno.max_heap_size_mb    if ...
SSR::Deno.isolate_pool_size   = config.ssr_deno.isolate_pool_size    if ...
SSR::Deno.node_builtins_enabled = config.ssr_deno.node_builtins_enabled if ...

# Missing:
# SSR::Deno.render_timeout_ms = config.ssr_deno.render_timeout_ms    if ...
```

`config.ssr_deno.render_timeout_ms` is also absent from the Railtie defaults block.
Rails users have no way to configure render timeout via `config/initializers/ssr_deno.rb`
(only via env var or direct `SSR::Deno.render_timeout_ms =` calls before bundle init).

**Fix:** Add to Railtie defaults:
```ruby
config.ssr_deno.render_timeout_ms = nil  # nil = 500ms (default)
```

And apply it in `init_bundles`:
```ruby
SSR::Deno.render_timeout_ms = config.ssr_deno.render_timeout_ms if config.ssr_deno.render_timeout_ms
```

---

## Bug 3 — `apply_integer_env` warning misleads on out-of-range values

**File:** `lib/ssr/deno.rb:142-145`  
**Severity:** Low

```ruby
begin
  send(setter, Integer(value))
rescue ArgumentError
  warn "[ssr-deno] Invalid integer for #{env_var}=#{value.inspect}, skipping"
end
```

`SSR_DENO_RENDER_TIMEOUT_MS=50` (below the 100ms minimum) parses as a valid integer, but
`native_set_render_timeout_ms(50)` raises `ArgumentError` ("Render timeout must be at least
100ms"). This is caught by the rescue and logged as "Invalid integer for
SSR_DENO_RENDER_TIMEOUT_MS=\"50\", skipping" — misleading: 50 is a valid integer, it's
out-of-range. The user sees no hint about the actual constraint.

**Fix:** Rescue with more context, or re-raise with a better message:
```ruby
rescue ArgumentError => e
  warn "[ssr-deno] Cannot apply #{env_var}=#{value.inspect}: #{e.message}, skipping"
end
```

---

## Bug 4 — `reload_if_changed` is not thread-safe under concurrent renders

**File:** `lib/ssr/deno/bundle.rb:132-138`  
**Severity:** Low (benign on MRI due to GVL, unsound in principle)

```ruby
def reload_if_changed
  current_mtime = File.mtime(@bundle_path)
  return unless current_mtime > @mtime
  reload
end

def reload
  @mtime = File.mtime(@bundle_path)
  ...
end
```

Two threads can both observe `current_mtime > @mtime` before either updates `@mtime`,
causing two concurrent `reload` calls. On MRI, the GVL serializes the Rust calls so
`native_load_bundle` is effectively called twice with the same args — idempotent on the
Rust side. But the double `File.mtime` syscall and double instrumentation event fire.

In JRuby/TruffleRuby (no GVL), `@mtime` reads and writes could race non-atomically.

**Fix:** Guard with a mutex or use a compare-and-set pattern:
```ruby
def reload_if_changed
  current_mtime = File.mtime(@bundle_path)
  return unless current_mtime > @mtime
  @reload_mutex.synchronize do
    return unless current_mtime > @mtime  # recheck inside lock
    reload
  end
end
```

For MRI-only use (which is the stated target), the current behavior is safe enough.
Minimum fix: add a `# not thread-safe: benign on MRI` comment so future maintainers
don't assume safety.

---

## Optimization 1 — `Dir.mktmpdir` without block leaks temp directories in tests

**Files:** `test/ssr/test_deno_render.rb:76-88`,
`test/ssr/test_deno_render_chunks.rb:95-143`  
**Severity:** Low (test-only resource leak)

Private helpers create temp dirs and never clean them up:

```ruby
def with_reject_bundle
  dir = Dir.mktmpdir        # no block → not cleaned up
  path = File.join(dir, ...)
  ...
  SSR::Deno::Bundle.new(path)
end
```

Each test run leaves directories in `/tmp`. Fix: use block form or add teardown cleanup:

```ruby
def with_reject_bundle
  @tmp_dirs ||= []
  dir = Dir.mktmpdir
  @tmp_dirs << dir
  ...
end

def teardown
  @tmp_dirs&.each { |d| FileUtils.rm_rf(d) }
  super
end
```

Or wrap the entire test body in `Dir.mktmpdir { |dir| ... }` where possible.

---

## Optimization 2 — Dead code in `scripts/performance.rb`

**File:** `scripts/performance.rb:196`, `249-253`  
**Severity:** Trivial

**Line 196:**
```ruby
SSR::Deno.isolate_pool_size # called for side effect if needed, but do not assign
```

`isolate_pool_size` is a pure getter (no side effects). This line does nothing. Remove it.

**Lines 249-253 (multi-thread `timings` array):**
```ruby
timings = []
...
Thread.new do
  count.times do
    tc = Process.clock_gettime(Process::CLOCK_MONOTONIC)
    bundle.render(...)
    timings << (Process.clock_gettime(Process::CLOCK_MONOTONIC) - tc)  # unsync'd write
  end
end
```

`timings` is populated by multiple threads without synchronization (unsafe concurrent
`Array#<<`), then never read — `sorted` and per-render stats are not computed for the
multi-thread mode. Either use the array (add `sorted = timings.sort` and print p50/p99
like single-thread mode) or remove it. The unsynchronized concurrent write is also a
race condition in non-MRI runtimes.

---

## Observation — `heap_stats` subscriber doesn't guard `enabled = false`

**File:** `lib/ssr/deno/rails/railtie.rb:49-68`

```ruby
initializer 'ssr_deno.heap_stats' do |_app|
  ActiveSupport::Notifications.subscribe('render.ssr_deno') do |*_args|
    ...
  end
end
```

Subscribes unconditionally regardless of `config.ssr_deno.enabled`. When disabled,
no `render.ssr_deno` events fire, so the subscription is dormant dead code. Harmless, but
inconsistent with `init_bundles` which checks `next unless config.ssr_deno.enabled`.

Add `next unless config.ssr_deno.enabled` at the start of the initializer block for
symmetry and clarity.

---

## Observation — `apply_bool_env` treats any non-truthy string as `false`

**File:** `lib/ssr/deno.rb:148-153`

```ruby
def apply_bool_env(env_var, setter)
  value = ENV.fetch(env_var, nil)
  return if value.nil? || value.empty?

  bool_value = %w[true 1 yes].include?(value.downcase)
  send(setter, bool_value)
end
```

`SSR_DENO_NODE_BUILTINS_ENABLED=garbage` → `bool_value = false` → silently applies
`false`. No warning is emitted for an unrecognised boolean string (unlike integer errors).
If the user typos `SSR_DENO_NODE_BUILTINS_ENABLED=treu`, node builtins silently stay
disabled.

**Fix:** Warn on unrecognised values:
```ruby
recognised = %w[true 1 yes false 0 no]
unless recognised.include?(value.downcase)
  warn "[ssr-deno] Unrecognised boolean for #{env_var}=#{value.inspect}, treating as false"
end
bool_value = %w[true 1 yes].include?(value.downcase)
```

---

## Status

| # | Item | File | Priority | Done |
|---|------|------|----------|------|
| 1 | `ssr_render` `.html_safe` on non-String | `helper.rb:26` | High | [x] |
| 2 | `render_timeout_ms` missing from Rails config | `railtie.rb` | Medium | [x] |
| 3 | `apply_integer_env` misleading out-of-range warning | `deno.rb:144` | Low | [x] |
| 4 | `reload_if_changed` thread-safety comment | `bundle.rb:132` | Low | [x] |
| 5 | `Dir.mktmpdir` temp dir leaks in tests | test files | Low | [x] |
| 6 | Dead code in `scripts/performance.rb` | `performance.rb` | Trivial | [x] |
| 7 | `heap_stats` subscriber misses `enabled` guard | `railtie.rb:49` | Trivial | [x] |
| 8 | `apply_bool_env` silent on unrecognised values | `deno.rb:148` | Low | [x] |
