# Code Review Fixes

Findings from a general codebase audit. Ordered by priority.

## Implementation Checklist

### HIGH

- [x] **render_stream.rs — propagate promise rejections as `DenoError::Render`**
  Promise rejection stored as `'ERROR:' + msg` in `__ssr_stream_result` → returned as
  successful `Ok(String)` → Ruby gets garbage instead of exception.
  Fix: use separate `globalThis.__ssr_stream_error` sentinel, check in poll loop,
  return `Err(DenoError::Render(...))`.

### MEDIUM

- [x] **mod.rs — cache `Box::leak`'d script names**
  Every `load_bundle` leaks a `&'static str`. Auto-reload in dev → unbounded leak per
  mtime change. Fix: static `Mutex<HashMap<String, &'static str>>` cache + `intern_script_name` helper.

- [x] **docs/architecture.md — update "NEVER runs event loop" section**
  Stale since event_loop/render_stream added. Needs caveat for `render_stream`/`event_loop: true` path.

- [x] **Rakefile comment — list all 5 test suites**
  Line 20 says "test:main, test:node_builtins" but there are 5 suites now.

### LOW

- [x] **call_render.rs — extract `extract_rejection_msg` helper**
  Duplicated pattern in phase1 + phase2 for reading promise rejection value.
  Implemented as a macro (`extract_rejection_msg!`) because V8's parameterized
  scope types don't unify into a single function signature.

- [x] **mod.rs — split `build_worker` (~100 lines) into smaller functions**
  Extracted `build_node_services` helper + `NodeServices` type alias.

- [x] **mod.rs `setup_require` — replace fixed 10ms deadline with poll-until-ready + 100ms cap**
  Now polls microtask checkpoint in a loop with 100ms safety cap and 50µs sleeps.
  Early exit via JS flag (`__ssr_require_ready`) not needed — verify step handles it.

- [x] **sig/ssr/deno.rbs — declare `native_get_*` methods**
  `native_get_max_heap_size_mb`, `native_get_isolate_pool_size`, `native_get_render_timeout_ms`,
  `native_get_node_builtins_enabled` added.

- [x] **call_render.rs — eliminate unsafe raw pointer for `v8::Global::new`**
  Replaced `unsafe { &*isolate_raw }` with `try_catch.as_ref()` (which provides `&Isolate`
  via `AsRef<Isolate>` impl on `PinnedRef<TryCatch<...>>`).

- [x] **render_stream.rs `op_ssr_push_chunk` — document silent drop + TODO for backpressure**
  Added inline comment documenting intentional silent drop and TODO for
  future `send().await` backpressure when true streaming is wired up.
