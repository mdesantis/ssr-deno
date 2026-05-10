# Codebase Review — Actionable Findings

_Excludes: already-documented TODOs, archived plans, known design tradeoffs._

---

## Rust — `ext/ssr_deno/src/`

### HIGH

**`builder.rs:127` — `unimplemented!()` panics the worker thread (not the process)**
The panic is contained to the V8 isolate's OS thread — the worker dies, reply
channel drops, Ruby gets `JsRuntimeWorkerError`. Added: explanatory comment
and a test (`test_web_worker_in_ssr_bundle_does_not_crash_process`).

### MEDIUM

**`lib.rs` — `Ruby::get().unwrap()` can panic off-Ruby-thread** ✅ Fixed
Threated `ruby: &Ruby` through all error helpers and native functions.
Removed all 3 `Ruby::get().unwrap()` sites. No more hidden panics.

**`lib.rs` + `mod.rs` — `Mutex::lock().unwrap()` poisoned-mutex risk** ✅ Fixed
Replaced all 10 sites with `unwrap_or_else(|e| e.into_inner())`. Added
`lock_config()` helper for the 8 CONFIG access sites. `POOL_INIT_LOCK` and
`SCRIPT_NAMES` inline recovery.

**`builder.rs:149-153` — OOM callback doubles heap limit without bound** ✅ Wontfix
Matches Deno's own pattern (`current_limit * 2` in tests). OOM terminates
execution immediately — the doubling is only needed for graceful V8 unwind.
No cap needed in practice. Added comment explaining rationale.

**`Cargo.toml:22` — Unused `transpile`/`hmr` features** ✅ `transpile` removed
`transpile`: removed — no remote module transpilation, all bundles pre-built.
`hmr`: kept — required by `deno_telemetry` (crashes with SyntaxError at boot).

### LOW

**`ssr_deno_core/src/lib.rs:33-44` — `SSRDenoError::Display` loses variant identity** ✅ Fixed
Each variant now prefixes its name: `Render: timeout`, `BundleLoad: not found`.

**`lib.rs:42-44` — `INITIALIZED` static is redundant**
Only `POOL_INIT_LOCK` (prevents double-init race) + `POOL` (OnceLock) are needed.
`INITIALIZED` duplicates `POOL.get().is_some()`.
Fix: remove `INITIALIZED`.

**`lib.rs:219` — unnecessary `.unwrap()` on just-set `OnceLock`**
`POOL.set(pool)` just succeeded, but `.unwrap()` is used instead of storing
the reference from `set`'s return value.
Fix: use `.expect("pool was just initialized")` or capture the return.

**Missing `// SAFETY:` comments**
- `lib.rs:36-40` — `render_worker` raw pointer protocol
- `lib.rs:17-24` — `rb_thread_call_without_gvl` FFI constraints
- `sys.rs:370` — `libc::isatty` raw fd
Fix: add `// SAFETY:` on each.

**`render_chunked.rs:83` — `inspect_err` requires Rust 1.83+**
No comment documenting the MSRV requirement.
Fix: add comment or use `map_err`.

**`render_chunked.rs:182-191` — silent JSON parse failure in `drain_chunks`**
```rust
if let Ok(chunks) = serde_json::from_str::<Vec<String>>(&json_str)
```
If V8 returns malformed JSON, chunks are silently dropped.
Fix: at minimum log the parse failure.

**`watchdog.rs:69-78` — `Drop` joins watchdog during panic unwind**
If worker thread panics mid-render, `Watchdog::drop` calls `handle.join()`
which blocks for up to `render_timeout_ms` during stack unwinding.
Fix: `handle.detach()` in Drop, or join with timeout.

**`nop_types.rs:71-72` — `/dev/null` path fails on Windows**
```rust
Url::from_file_path("/dev/null").expect("Valid file path")
```
Fix: use a platform-agnostic path or note Unix-only.

**`Cargo.toml:28` — `libc` dep could be replaced**
`libc::isatty` → `std::io::IsTerminal` (stable since Rust 1.70).
Fix: remove `libc` dependency, use std instead.

**`handle.rs:32` — channel capacity 1 causes head-of-line blocking**
`HeapStats` request blocks behind any running render. For pool of N isolates,
`heap_stats()` can be N render durations behind.
Fix: separate low-priority channel, or document the limitation.

---

## Ruby — `lib/` and `sig/`

### HIGH

**None.**

### MEDIUM

**`lib/ssr/deno.rb:111` — `heap_stats!` raises `JSON::ParserError`**
Not a `SSR::Deno::Error` descendant. Callers catching `SSR::Deno::Error`
would miss it.
Fix: wrap `JSON.parse` and convert to `HeapStatsSerializationError`.

**`lib/ssr/deno/bundle.rb:145-151` — thread-unsafe `@mtime`**
`reload_if_changed` reads `@mtime` while `reload` writes it. No synchronization.
Comment acknowledges "benign on MRI (GVL serializes)" but this is unsound on
JRuby/TruffleRuby.
Fix: `Mutex` around the read-check-write, or `MonitorMixin`.

**`lib/ssr/deno/bundle.rb:56,87,121,148` — thread-unsafe `@auto_reload`**
Read/written across threads without synchronization.
Fix: same as above — synchronize or document.

**`lib/ssr/deno/instrumenter.rb:20` — `yield` without block guard**
```ruby
def instrument(...)
  return yield(...) unless defined?(ActiveSupport::Notifications)
```
If called without a block in no-AS mode, raises `LocalJumpError`.
Fix: `yield if block_given?`.

**`lib/ssr/deno/rails/helper.rb:57-73` — thread-unsafe `@registry` mutation**
`find_bundle!` calls `create_bundles!` which does `@registry.transform_values!`
while another thread could be reading `@registry`.
Fix: synchronize registry access or use `Concurrent::Hash`.

**`lib/ssr/deno/rails/helper.rb:22` — unknown options silently ignored**
`ssr_render(data, **options)` passes all kwargs to `bundle.render()`. If the
caller has a typo like `raw_ouputput: true`, it's silently ignored with no
warning.
Fix: validate known keys (`:bundle`, `:raw_input`, `:raw_output`), warn on unknown.

**`lib/ssr/deno/rails/install_generator.rb:14-23` — duplicate Puma content**
`append_to_file 'config/puma.rb'` runs every time the generator runs.
Running `rails generate ssr:deno:install` twice = duplicate `on_worker_boot` blocks.
Fix: check for existing content before appending.

**`lib/ssr/deno/rails/install_generator.rb:15` — overwrites existing Puma config**
`create_file 'config/puma.rb'` destroys any existing Puma config.
Fix: use `inject_into_file` with sentinel, or `create_file` only if missing.

**`sig/ssr/deno.rbs:32` — `Instrumenter.instrument` block signature**
```rbs
def self.instrument: (untyped name, ?::Hash[untyped, untyped] payload) { () -> untyped } -> untyped
```
The block receives a payload argument. Should be `{ (Hash[untyped, untyped]) -> untyped }`.
Fix: add payload arg to block type.

**`lib/ssr/deno/ractor_pool.rb:61` — inconsistent return value**
`pool.render_chunks(data) { |c| ... }` returns `Array[String]` while
`Bundle#render_chunks` returns `nil` with a block.
Fix: return `nil` when block given (match Bundle behavior).

**`lib/ssr/deno/ractor_pool.rb:77` — `shutdown` can block forever**
If a worker is hung in `native_render`, `ractor_result(w)` blocks indefinitely.
Fix: add timeout or use `Ractor::Selector`.

**`lib/ssr/deno/ractor_pool.rb:78` — `shutdown` swallows exceptions**
```ruby
rescue StandardError
  nil
```
If a worker crashed, caller never knows.
Fix: at minimum log the error.

### LOW

**`lib/ssr/deno.rb:157` — British spelling**
`"Unrecognised boolean"` vs US `"initialization"` elsewhere — minor inconsistency.

**`lib/ssr/deno/bundle.rb:44` — `@bundle_id` naming**
`@bundle_id = @bundle_path` — the word "id" suggests a synthetic identifier,
but it's just the file path.
Fix: rename to `@bundle_path` or clarify in comment.

**`lib/ssr/deno/ractor_pool.rb:146` — `@counter` overflow**
Increments forever in signed 64-bit. ~9 quintillion renders to overflow.
Not actionable, but worth noting.

**Various RBS imprecisions** (lines 99-100, 116, 90, 156):
- `next_worker` return type `untyped` → `Ractor`
- `@auto_reload` type `boolish` → `bool` (never nil)
- Minor, no runtime impact.

---

## Tests — `test/`

### HIGH

**`test/ssr/test_deno_bundle.rb:160-191` — race-prone busy-wait** ✅ Fixed
Replaced `Thread#status == 'sleep'` busy-loop with `Queue` signaling.

### MEDIUM

**`test/ssr/test_integration_puma.rb` — orphaned child processes**
`spawn` with `out: '/dev/null', err: '/dev/null'` silences startup errors.
Test interruption leaves Puma orphaned.
Fix: capture output, ensure PID cleanup on all exit paths.

**`test/ssr/test_integration_puma.rb:129-137` — unsafe HTTP response parsing**
```ruby
raw.lines.first.split[1]
raw.split("\r\n\r\n", 2).last
```
Assumes simple response format. Brittle.
Fix: use `Net::HTTP` or a proper HTTP parser.

**`test/ssr/test_deno_errors.rb:21-29` — test name contradicts assertion**
Name says `bundle_not_found_error` but assertion expects `Errno::ENOENT`.
Fix: rename test or fix assertion.

**`test/ssr/test_deno_render_timeout.rb:54-55` — flaky wall-clock timing**
```ruby
elapsed_ms = ((Time.now - start) * 1000).to_i
```
On busy CI, actual elapsed can exceed 500ms. Tight range (80-500ms).
Fix: use wider upper bound (e.g., 2000ms) or `CLOCK_MONOTONIC`.

**`test/ssr/test_integration_hmr.rb:30-33` — source file corruption risk**
If test crashes before setting `@original_src`, teardown writes `nil` to source file.
Fix: guard `File.write` with `if @original_src`.

**`test/ssr/test_integration_hmr.rb:43-51` — implicit `deno` PATH dependency**
`system('deno', 'task', 'build', chdir: ...)` fails silently if `deno` not on PATH.
Fix: skip test if `deno` unavailable, or use configured path.

### LOW

- `test/ssr/test_perf.rb` — single method bundles all benchmarks, poor failure isolation
- `test/ssr/test_deno_concurrency.rb:23` — uses `instance_variable_get` instead of public accessor
- `test/ssr/test_deno_render.rb:42` — inconsistent `.to_json` vs `JSON.generate`
- `test/support/subprocess_helper.rb` — doesn't capture subprocess stderr on failure
- `test/support/perf_helpers.rb` — arbitrary thresholds (1.5x, 1.3x) may be flaky on single-core CI
- `test/ssr/test_deno_stability.rb:16-22` — 3x heap growth allowance is generous, GC timing dependent
- `test/ssr/test_deno_macrotasks.rb:63-69` — temp bundle destroyed during test, may confuse pool
