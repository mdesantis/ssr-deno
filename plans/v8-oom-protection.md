# V8 OOM Protection

## Problem

When a user's SSR component leaks memory across renders, V8's heap eventually
hits the `max_heap_size_mb` limit. At that point, V8 does NOT throw a catchable
JS exception — it calls `V8::FatalError("Reached heap limit")` and `abort()`s
the entire Ruby process.

Reproduction script: [`attachments/reproduce_v8_oom.rb`](attachments/reproduce_v8_oom.rb)

Output from a run with `max_heap_size_mb = 16` and a 500 KB leak per render:

```
<--- Last few GCs --->
Mark-Compact (reduce) 15.3 (16.1) -> 15.3 (16.1) MB, last resort; GC in old space requested
Mark-Compact (reduce) 15.3 (16.1) -> 15.3 (16.1) MB, last resort; GC in old space requested

#
# Fatal JavaScript out of memory: Reached heap limit
#
==== C stack trace ===============================
[... V8 internals ...]
EXIT CODE: 133  (128 + SIGTRAP)
```

V8 runs desperate last-resort GCs at ~15.3/16.1 MB, fails to free the leaky
array, and aborts. No JS exception, no Rust `Result`, no Ruby `rescue`. Just
`SIGTRAP` + core dump.

## Why a Ruby-side watchdog can't work

| Obstacle | Detail |
|---|---|
| **No abort API** | `native_render` is a synchronous blocking call through `blocking_send → worker → V8.execute_script → renderToString`. Nothing outside that call chain can interrupt it. |
| **Heap stats blind** | `SSR::Deno.heap_stats` samples a single isolate via round-robin (`next_handle`). The isolate about to OOM may not be the one sampled. No per-isolate heap stats exposed yet. |
| **Race** | Between "detect 90% heap usage" and "try to do something", V8's allocator already tripped. The render call is mid-execution — the heap limit is hit inside `execute_script`. |

## How Deno itself handles this

Deno uses a combination of `add_near_heap_limit_callback` + `terminate_execution`
to prevent the fatal OOM. This approach is already implemented in `deno_core`
(the crate we depend on):

```rust
// From deno_core-0.399.0/runtime/tests/misc.rs (test_heap_limits)
let cb_handle = runtime.v8_isolate().thread_safe_handle();

runtime.add_near_heap_limit_callback(move |current_limit, _initial_limit| {
    inner_invoke_count.fetch_add(1, Ordering::SeqCst);
    cb_handle.terminate_execution();
    current_limit * 2  // double the limit to buy one more GC cycle
});

let js_err = runtime
    .execute_script("script name", r#"let s = ""; while(true) { s += "Hello"; }"#)
    .expect_err("script should fail");

assert_eq!(
    "Uncaught Error: execution terminated",
    js_err.exception_message
);
```

**How it works:**

```mermaid
flowchart TD
    GC["V8 GC detects heap near limit"] --> CB["near_heap_limit_callback fires"]
    CB --> Double["returns current_limit * 2\n(buys one extra GC cycle)"]
    CB --> Terminate["isolate_handle.terminate_execution()"]
    Double --> V8gc2["V8 GC re-runs with new limit"]
    Terminate --> V8gc2
    V8gc2 -->|"heap freed"| OK["continue normally"]
    V8gc2 -->|"still over limit"| TermCheck["V8 checks termination flag"]
    TermCheck --> Err["execute_script → Err(JsError)\n'Uncaught Error: execution terminated'"]
    Err -.-> Rust["call_render maps to RenderError"]
```

**Key insight:** The callback does two things:
1. **Doubles the limit** — prevents the immediate fatal OOM long enough for V8 to check the termination flag
2. **Terminates execution** — stops the running JS code, turning a crash into a catchable error

Deno's `web_worker.rs` also tracks whether the callback fired via an
`oom_triggered: Arc<AtomicBool>` flag, so it can report a specific
`ERR_WORKER_OUT_OF_MEMORY` error instead of the generic "execution terminated".

**What this means for us:** We can adopt the same approach in `build_worker`
(`ext/ssr_deno/src/deno_runtime_wrapper/mod.rs:432`). The `MainWorker` wraps a
`JsRuntime`, which exposes `add_near_heap_limit_callback()` and
`v8_isolate().thread_safe_handle()`. All the primitives are already available in
our dependency chain — we just need to wire them up.

## Three levels of Rust-side defense

### Level 1: Pre-render heap threshold (low effort, high value)

Before dispatching a render, check the target isolate's `used_heap_size / heap_size_limit`. If the ratio exceeds a configurable threshold (e.g., 85%):

1. Call `isolate.low_memory_notification()` — triggers V8's aggressive GC
2. Re-check the ratio
3. If still above threshold, refuse the render with a `RenderError` (not a crash)

```mermaid
flowchart TD
    Render["render request arrives"] --> Check["used / limit > threshold?"]
    Check -->|no| Proceed["proceed with render"]
    Check -->|yes| GC["isolate.low_memory_notification()"]
    GC --> Recheck["used / limit > threshold?"]
    Recheck -->|no| Proceed
    Recheck -->|yes| Reject["return RenderError:\n'Isolate heap near limit'"]
```

**What it prevents:** Starting a render when the heap is already critical. A
render that allocates moderately on a healthy heap won't tip over. One that
allocates heavily on an already-near-limit heap will be rejected before it can
trigger the fatal OOM.

**What it does NOT prevent:** A single render that allocates ~10 MB in one
shot on a heap at 50% usage (50% → 100% within one `renderToString`). The
threshold check passes, but the render itself can still hit the limit. Mitigated
by: configure `max_heap_size_mb` with enough headroom for the heaviest single
render.

**Implementation:**
- New `SSR::Deno.oom_threshold=` config (default `0.0` = disabled, range `0.0–1.0`)
- Passed to worker as a new field in `WorkerMsg::Render` (or as a worker-level field set at spawn)
- Pre-render check in `call_render` at `ext/ssr_deno/src/deno_runtime_wrapper/call_render.rs:16`, before scope chain is entered
- `heap_size_limit` typically equals `max_heap_size_mb * 1024 * 1024` (the configured cap)

### Level 2: Near-heap-limit callback + `terminate_execution` (medium effort, Deno-proven)

This is the approach Deno's own `test_heap_limits` test uses, and `deno_core`
exposes it as a public API (`JsRuntime::add_near_heap_limit_callback`).
It prevents the fatal OOM by combining two V8 primitives in a single callback:

```mermaid
flowchart TD
    GC["V8 GC detects heap near limit"] --> CB["near_heap_limit_callback"]
    CB --> Double["returns current_limit * 2"]
    CB --> Term["thread_safe_handle.terminate_execution()"]
    Double --> Check["V8 re-checks heap after GC"]
    Term --> Check
    Check -->|"freed enough"| OK["continue normally"]
    Check -->|"still over limit"| Abort["execute_script → JsError\n'execution terminated'"]
    Abort -.-> Rust["call_render → RenderError"]
```

The callback:
1. **Doubles the heap limit** — buys one more GC cycle, preventing the
   immediate fatal `SIGTRAP`
2. **Terminates execution** via `isolate.thread_safe_handle().terminate_execution()`
   — makes V8 throw "Uncaught Error: execution terminated" instead of aborting

The termination error propagates through `call_render` as a normal JS exception
and maps to `DenoError::Render → SSR::Deno::RenderError` at the Ruby level —
same error path as a JS `throw`.

**Integration into `build_worker`:**
- After creating the `MainWorker`, call `worker.js_runtime.add_near_heap_limit_callback(...)`
- The callback captures `worker.js_runtime.v8_isolate().thread_safe_handle()`
- `MainWorker.js_runtime` is public — no architectural changes needed

**Optional improvement — `oom_triggered` flag:**
Deno's `web_worker.rs` tracks whether the callback fired via an `AtomicBool`,
so the error message can say "heap memory limit exceeded" instead of the
generic "execution terminated". We can add the same flag to `call_render`.

**What it catches:** Any render that triggers near-heap-limit, including a
single heavy render that quickly fills the heap. No pre-render threshold needed.

**What it costs:** One callback registration per isolate at `build_worker` time.
Negligible runtime overhead until GC runs near the limit.

### Level 3: `set_oom_error_handler` (low value, exploratory)

V8 also exposes `Isolate::set_oom_error_handler()`, which fires right before
the fatal OOM abort. Unlike `near_heap_limit_callback`, it cannot prevent the
abort — it only logs context. Deno does not use it. Not planned for
implementation.

## Recommended approach: Level 2 (Deno-proven)

Level 2 gives full OOM protection with minimal code, using primitives already
available in our dependency chain (`deno_core::JsRuntime::add_near_heap_limit_callback`
+ `rusty_v8::IsolateHandle::terminate_execution`). Deno's own test suite
validates this exact pattern. No need for the pre-render threshold (Level 1)
when Level 2 catches the same cases and more.

## What's NOT affected

- **Renders on other isolates** — an OOM crash in isolate-3 does NOT affect isolates 0,1,2 (separate V8 isolates, separate heaps). Only the process dies, taking all isolates with it.
- **Non-render operations** — `load_bundle_in_worker` (bundle load) and `collect_heap_stats` (heap query) don't allocate significant JS objects and won't trigger OOM.

## Implementation plan (Level 2)

### [x] Step 1: Register near-heap-limit callback in `build_worker`

**File:** `ext/ssr_deno/src/deno_runtime_wrapper/mod.rs`

After `MainWorker::bootstrap_from_options` and before returning, register the
callback on the worker's `JsRuntime`:

```rust
let thread_safe_handle = worker.js_runtime.v8_isolate().thread_safe_handle();

worker.js_runtime.add_near_heap_limit_callback(
    move |current_limit, _initial_limit| {
        // Suppress the fatal OOM: double the limit to buy one more GC
        // cycle, then terminate execution to turn the crash into a
        // catchable error.
        let _ = thread_safe_handle.terminate_execution();
        current_limit * 2
    },
);
```

The `terminate_execution()` call makes V8 throw `"Uncaught Error: execution terminated"`
instead of aborting. This error propagates through `call_render` as a normal
JS exception and maps to `DenoError::Render`.

The `current_limit * 2` return value gives V8 one more GC cycle to try to
free memory. If still over after GC, the termination flag is already set and
`execute_script` returns the error.

### [ ] Step 2: Improve error message (optional)

Optionally add an `oom_triggered: AtomicBool` flag to the worker or to
`call_render` context, set it in the callback, and check it after `execute_script`
returns the termination error. This lets us return `"JS heap out of memory"` instead
of the generic `"Error: execution terminated"`.

### [x] Step 3: Verify

`bundle exec rake` passes — compile, cargo test, sample builds, all Ruby suites,
RuboCop, 100% coverage.

Rerun `attachments/reproduce_v8_oom.rb` — verify it now produces a Ruby error
(`SSR::Deno::RenderError`) instead of `SIGTRAP` + core dump.

## Files Changed

| File | Change |
|---|---|---|
| `ext/ssr_deno/src/deno_runtime_wrapper/mod.rs` | Register `add_near_heap_limit_callback` in `build_worker` |
| `test/ssr/test_deno_stability.rb` | OOM subprocess test `test_oom_produces_render_error` |
| `test/fixtures/large-payload-bundle.js` | Fixture for large-payload test |

## Files NOT Changed

| File | Reason |
|---|---|
| `ext/ssr_deno/crates/ssr_deno_core/src/lib.rs` | No new config needed — callback is always enabled |
| `ext/ssr_deno/src/deno_runtime_wrapper/call_render.rs` | Termination error already propagates through existing error handling |
| `lib/ssr/deno.rb` | No API changes |
| `sig/ssr/deno.rbs` | No type changes |
| `lib/ssr/deno/rails/railtie.rb` | No Rails config needed |
| `README.md` / `CHANGELOG.md` | Deferred until feature lands |

## Verification

- `bundle exec rake` exits 0
- `attachments/reproduce_v8_oom.rb` produces `SSR::Deno::RenderError` instead of SIGTRAP
- Existing tests continue to pass (termination callback only fires on near-OOM)
