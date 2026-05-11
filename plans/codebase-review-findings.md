# Codebase Review — Actionable Findings

_Excludes: already-documented TODOs, archived plans, known design tradeoffs._

---

## Rust — `ext/ssr_deno/src/`

### HIGH

**`builder.rs:127` — `unimplemented!()` panics the worker thread (not the process)** ✅ Wontfix
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

**`lib.rs:42-44` — `INITIALIZED` static is redundant** ✅ Fixed
Removed `INITIALIZED` static, replaced `INITIALIZED.get().is_some()` with
`POOL.get().is_some()`. Also slightly more correct: eliminates a race window
between `POOL.set()` and `INITIALIZED.set()`.

**`lib.rs:219` — unnecessary `.unwrap()` on just-set `OnceLock`** ✅ Fixed
Replaced `.unwrap()` with `.expect("pool was just initialized")`.

**Missing `// SAFETY:` comments** ✅ Fixed (2 added, 1 was already present)
- `lib.rs:43` — `render_worker`: raw pointer protocol + GVL-free constraints
- `lib.rs:24` — `rb_thread_call_without_gvl`: FFI constraints
- `sys.rs:369` — `libc::isatty`: already had `// SAFETY:` comment

**`render_chunked.rs:83` — `inspect_err` requires Rust 1.83+** ✅ Fixed
Added MSRV comment. No code change needed (current toolchain is 1.95).

**`render_chunked.rs:182-191` — silent JSON parse failure in `drain_chunks`** ✅ Wontfix
Already has a thorough comment (lines 178-183) explaining why: V8's JSON.stringify
cannot produce invalid JSON for a well-formed array. A corrupt V8 heap is the
only theoretical path, and logging from within deno_core Ops is impractical.

**`watchdog.rs:69-78` — `Drop` joins watchdog during panic unwind** ✅ Fixed
Changed `Drop` to drop the `JoinHandle` instead of joining — detaches the
watchdog thread during panic unwind. Normal path (`cancel()`) still joins.

**`nop_types.rs:71-72` — `/dev/null` path fails on Windows** ✅ Fixed
Replaced `from_file_path("/dev/null")` with `parse("file:///dev/null")`.
Added comment noting Unix-only constraint.

**`Cargo.toml:28` — `libc` dep could be replaced** ✅ Fixed
Replaced `libc::isatty(fd)` with `File::is_terminal()` (std::io::IsTerminal).
Removed `libc` dependency from Cargo.toml.

**`handle.rs:32` — channel capacity 1 causes head-of-line blocking**
`HeapStats` request blocks behind any running render. For pool of N isolates,
`heap_stats()` can be N render durations behind.
Fix: separate low-priority channel, or document the limitation.

---

## Ruby — `lib/` and `sig/`

### HIGH

**None.**

### MEDIUM

**`lib/ssr/deno.rb:111` — `heap_stats!` raises `JSON::ParserError`** ✅ Fixed
Wrapped `JSON.parse` in rescue converting to `HeapStatsSerializationError`.
Also added `HeapStatsSerializationError` to `heap_stats` rescue list.

**`lib/ssr/deno/bundle.rb:145-151` — thread-unsafe `@mtime`** ✅ Fixed
`Mutex` guards `@mtime` read-check-write in `reload_if_changed` and write in `reload`.

**`lib/ssr/deno/bundle.rb:56,87,121,148` — thread-unsafe `@auto_reload`** ✅ Fixed
`attr_reader :auto_reload` (zero-overhead read on hot path), `auto_reload=` synchronizes on `@_bundle_mutex`.

**`lib/ssr/deno/instrumenter.rb:20` — `yield` without block guard** ✅ Fixed
Changed `else yield` to `elsif block_given? yield payload`. No-AS mode now yields
the payload hash (matching AS behaviour) and is safe without a block.
`bundle.rb:105` guard `if payload` removed — payload always truthy after fix.

**`lib/ssr/deno/rails/helper.rb:57-73` — thread-unsafe `@registry` mutation** ✅ Fixed
`find_bundle!` now calls `create_bundles!` first (idempotent, returns immediately
after first run), then reads. Read always happens after mutation completes — no
concurrent read-during-transform_values! window.

**`lib/ssr/deno/rails/helper.rb:22` — unknown options silently ignored** ✅ Fixed
`assert_known_ssr_render_options!` validates options after `:bundle` is removed.
Unknown keys raise `ArgumentError` with names listed before the render attempt.

**`lib/ssr/deno/rails/install_generator.rb:14-23` — duplicate Puma content** ✅ Fixed
`add_puma_on_worker_boot` now checks for the sentinel string before appending.
Idempotent: re-running the generator is a no-op when block already present.

**`lib/ssr/deno/rails/install_generator.rb:15` — overwrites existing Puma config** ✅ Fixed
Removed unconditional `create_file`. File created only when missing (`unless
File.exist?`). Existing puma.rb is preserved; on_worker_boot block is appended.

**`sig/ssr/deno.rbs:32` — `Instrumenter.instrument` block signature** ✅ Fixed
Block type updated to `?{ (::Hash[untyped, untyped]) -> untyped }` (optional block,
payload arg). Same fix applied to `Bundle#instrument` private sig.

**`lib/ssr/deno/ractor_pool.rb:61` — inconsistent return value** ✅ Fixed
`render_chunks` with block now returns `nil` (matches `Bundle#render_chunks`).
RBS updated: block overload return type `Array[String]` → `nil`.

**`lib/ssr/deno/ractor_pool.rb:77` — `shutdown` can block forever** ✅ Fixed
Removed `ractor_result(w)` — shutdown is now fire-and-forget. `:shutdown`
message is sent; workers terminate after their current render completes.

**`lib/ssr/deno/ractor_pool.rb:78` — `shutdown` swallows exceptions** ✅ Fixed
`rescue StandardError => error` now warns to STDERR instead of silently
returning nil. Worker send failures are visible in logs.

### LOW

**`lib/ssr/deno.rb:157` — British spelling**
`"Unrecognised boolean"` vs US `"initialization"` elsewhere — minor inconsistency.

**`lib/ssr/deno/bundle.rb:44` — `@bundle_id` naming** ✅ Fixed
Removed `@bundle_id` — it was always equal to `@bundle_path`. All usages
replaced with `@bundle_path` directly. RBS `@bundle_id` ivar removed.

**`lib/ssr/deno/ractor_pool.rb:146` — `@counter` overflow**
Increments forever in signed 64-bit. ~9 quintillion renders to overflow.
Not actionable, but worth noting.

**Various RBS imprecisions** (lines 99-100, 116, 90, 156): ✅ Fixed
- `next_worker` return type `untyped` → `Ractor`
- `RactorPool#@auto_reload` type `boolish` → `bool` (never nil)
- `initialize` `auto_reload:` param `boolish` → `bool`

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

**`test/ssr/test_deno_errors.rb:21-29` — test name contradicts assertion** ✅ Fixed
Renamed to `test_bundle_initialize_when_path_not_found_raises_errno_enoent`.
`Bundle.new` raises `Errno::ENOENT` (from `File.mtime`) not `BundleNotFoundError`.

**`test/ssr/test_deno_render_timeout.rb:54-55` — flaky wall-clock timing** ✅ Fixed
`Time.now` → `Process.clock_gettime(Process::CLOCK_MONOTONIC)`. Upper bound
500ms → 2000ms to accommodate busy CI without losing the core timing assertion.

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
