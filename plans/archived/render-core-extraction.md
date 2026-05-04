# render_core — extract shared event-loop skeleton

Status: Pending

## Problem

~90 LOC of boilerplate is duplicated verbatim between `render::render()` and
`render_chunked::render_chunked()`. Every change to the watchdog setup, error
dispatch, or teardown must be edited in both files — an ongoing maintenance
burden that has already caused one inconsistency (the missing global cleanup).

## Duplication Map

| Section | render.rs lines | render_chunked.rs lines |
|---------|----------------|------------------------|
| `bundle_id_js` / `args_json_js` serialization | 80–93 | 44–47 |
| Watchdog arm | 120–124 | 78–81 |
| `execute_script` + error dispatch | 126–152 | 83–107 |
| Event loop tick structure | 157–180 | 111–139 |
| Watchdog cancel + `cancel_terminate_execution` | 170–177 | 141–148 |

Only the script template body and the per-tick behaviour differ.

## Scope

The event loop (`run_up_to_duration` + OOM/timeout check + `poll_render_state`)
cannot be cleanly extracted because:

- `drain_chunks` in the chunked path uses `.await` — requires an async closure
  signature (`Fn(&mut MainWorker) -> Fut` where `Fut: Future`)
- The two functions have different return types (`String` vs `()`)
- The chunked path needs a final `drain_chunks` after `Done` that the buffered
  path doesn't

Extracting would save ~25 LOC per file but add a generic async closure
signature that's harder to read than the duplication. **Not worth it.**

Extract everything else:

| Extract | Lines saved per file |
|---------|---------------------|
| `to_js_string()` helper | ~6 |
| `begin_render()` — watchdog arm + exec + error dispatch | ~35 |
| `end_render()` — watchdog cancel + terminate clearance | ~10 |
| **Total** | **~50** |

## Implementation Draft

### Helper: `to_js_string`

Add to `render.rs`:

```rust
/// Produces a JS-safe string literal from a bundle_id or args_json.
/// Uses serde_json for guaranteed escaping, falls back to double-quoting.
pub(super) fn to_js_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| format!("\"{}\"", s))
}
```

Then replace in both files:

```rust
// Before (render.rs 80-93, render_chunked.rs 44-47):
let bundle_id_js = serde_json::to_string(bundle_id)
    .unwrap_or_else(|_| format!("\"{}\"", bundle_id));
let args_json_js = serde_json::to_string(args_json)
    .unwrap_or_else(|_| format!("\"{}\"", args_json));

// After:
let bundle_id_js = to_js_string(bundle_id);
let args_json_js = to_js_string(args_json);
```

### Helper: `begin_render`

Add to `render.rs`:

```rust
/// Arms the watchdog, executes `startup_script`, and dispatches execution
/// errors (OOM, timeout, BundleNotFound, generic Render). Returns the
/// watchdog and timeout flag on success.
pub(super) fn begin_render(
    worker: &mut MainWorker,
    startup_script: String,
    render_timeout_ms: u64,
    oom_triggered: &AtomicBool,
    error_label: &str,
) -> Result<(Watchdog, Arc<AtomicBool>), SSRDenoError> {
    let v8_handle = worker.js_runtime.v8_isolate().thread_safe_handle();
    let timeout_triggered = Arc::new(AtomicBool::new(false));
    let watchdog = Watchdog::spawn(v8_handle, render_timeout_ms, timeout_triggered.clone());

    let exec_result = worker.execute_script(
        &format!("<ssr-deno:{error_label}-start>"),
        startup_script.into(),
    );

    if let Err(e) = exec_result {
        watchdog.cancel();
        cleanup_render_globals(worker);

        if oom_triggered.load(Ordering::SeqCst) {
            worker.js_runtime.v8_isolate().cancel_terminate_execution();
            return Err(SSRDenoError::OutOfMemory(
                format!("{error_label} - JS heap out of memory"),
            ));
        }
        if timeout_triggered.load(Ordering::SeqCst) {
            worker.js_runtime.v8_isolate().cancel_terminate_execution();
            return Err(SSRDenoError::Render(
                format!("{error_label} timed out"),
            ));
        }

        let msg = e.to_string();
        return if msg.contains("Bundle not found:") {
            Err(SSRDenoError::BundleNotFound(msg))
        } else {
            Err(SSRDenoError::Render(
                format!("{error_label} failed to start: {msg}"),
            ))
        };
    }

    Ok((watchdog, timeout_triggered))
}
```

Replace in both files:

```rust
// Before (render.rs 120-152, render_chunked.rs 78-107):
// (18 LOC of watchdog arm + 30 LOC of error dispatch)

// After:
let (watchdog, timeout_triggered) = begin_render(
    worker, script, render_timeout_ms, oom_triggered, "render",
)?;
```

### Helper: `end_render`

Add to `render.rs`:

```rust
/// Cancels the watchdog and clears any pending terminate_execution.
pub(super) fn end_render(
    worker: &mut MainWorker,
    watchdog: Watchdog,
    timeout_triggered: &AtomicBool,
    oom_triggered: &AtomicBool,
) {
    watchdog.cancel();
    if timeout_triggered.load(Ordering::SeqCst) || oom_triggered.load(Ordering::SeqCst) {
        worker.js_runtime.v8_isolate().cancel_terminate_execution();
    }
}
```

Replace in both files:

```rust
// Before (render.rs 170-177, render_chunked.rs 141-148):
watchdog.cancel();
if timeout_triggered.load(Ordering::SeqCst) || oom_triggered.load(Ordering::SeqCst) {
    worker.js_runtime.v8_isolate().cancel_terminate_execution();
}

// After:
end_render(worker, watchdog, &timeout_triggered, oom_triggered);
```

### Final `render()` after extraction

```rust
pub async fn render(
    worker: &mut MainWorker,
    bundle_id: &str,
    args_json: &str,
    render_timeout_ms: u64,
    oom_triggered: &AtomicBool,
) -> Result<String, SSRDenoError> {
    let bundle_id_js = to_js_string(bundle_id);
    let args_json_js = to_js_string(args_json);

    let script = build_render_script(&bundle_id_js, &args_json_js);

    let (watchdog, timeout_triggered) = begin_render(
        worker, script, render_timeout_ms, oom_triggered, "render",
    )?;

    let result = event_loop(worker, oom_triggered, &timeout_triggered, |_w| Ok(())).await;

    end_render(worker, watchdog, &timeout_triggered, oom_triggered);
    cleanup_render_globals(worker);

    result
}
```

Where `event_loop` is the existing inline loop (kept as-is, not extracted).

### Final `render_chunked()` after extraction

```rust
pub async fn render_chunked(
    worker: &mut MainWorker,
    bundle_id: &str,
    args_json: &str,
    render_timeout_ms: u64,
    chunk_tx: mpsc::Sender<String>,
    oom_triggered: &AtomicBool,
) -> Result<(), SSRDenoError> {
    let bundle_id_js = to_js_string(bundle_id);
    let args_json_js = to_js_string(args_json);

    let script = build_chunked_script(&bundle_id_js, &args_json_js);

    let (watchdog, timeout_triggered) = begin_render(
        worker, script, render_timeout_ms, oom_triggered, "chunked-render",
    )?;

    let result = event_loop(worker, oom_triggered, &timeout_triggered, |w| {
        drain_chunks(w, &chunk_tx).await;
        Ok(())
    }).await;

    if result.is_ok() {
        drain_chunks(worker, &chunk_tx).await;
    }

    end_render(worker, watchdog, &timeout_triggered, oom_triggered);

    let _ = worker.execute_script(
        "<ssr-deno:render-chunked-cleanup>",
        "globalThis.__ssr_chunks = undefined; globalThis.__ssr_push_chunk = undefined;"
            .to_string().into(),
    );

    drop(chunk_tx);

    result.map(|_| ())
}
```

## Test Strategy

No new functional tests needed — pure refactoring. All existing tests
must continue to pass:

- `test/ssr/test_deno_render.rb` — buffered render
- `test/ssr/test_deno_render_chunks.rb` — chunked render
- `test/ssr/test_deno_render_timeout.rb` — timeout behaviour
- `test/ssr/test_deno_concurrency.rb` — concurrent access
- `test/ssr/test_deno_stability.rb` — OOM / leak detection

Run `bundle exec rake` after each extraction step.

## Verification

- [x] Add `to_js_string()` helper and use in both files
- [x] Extract `begin_render()` — watchdog arm + script execute + error dispatch
- [x] Extract `end_render()` — watchdog cancel + terminate clearance
- [x] Refactor `render()` to use shared helpers
- [x] Refactor `render_chunked()` to use shared helpers
- [x] `bundle exec rake` — must exit 0
- [x] Verify test results match pre-refactoring
