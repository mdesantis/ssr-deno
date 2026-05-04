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

- [ ] **docs/architecture.md — update "NEVER runs event loop" section**
  Stale since event_loop/render_stream added. Needs caveat for `render_stream`/`event_loop: true` path.

- [ ] **Rakefile comment — list all 5 test suites**
  Line 20 says "test:main, test:node_builtins" but there are 5 suites now.

### LOW

- [ ] **call_render.rs — extract `extract_rejection_msg` helper**
  Duplicated pattern in phase1 + phase2 for reading promise rejection value.

- [ ] **mod.rs — split `build_worker` (~100 lines) into smaller functions**
  Extract `build_node_services`, `build_worker_options` helpers.

- [ ] **mod.rs `setup_require` — replace fixed 10ms deadline with poll-until-ready + 100ms cap**
  Current arbitrary 10ms may fail on cold start. Poll `typeof globalThis.require` per
  microtask checkpoint, early-exit on success, generous 100ms cap.

- [ ] **sig/ssr/deno.rbs — declare `native_get_*` methods**
  `native_get_max_heap_size_mb`, `native_get_isolate_pool_size`, `native_get_render_timeout_ms`,
  `native_get_node_builtins_enabled` missing from RBS.

- [ ] **call_render.rs — eliminate unsafe raw pointer for `v8::Global::new`**
  Restructure scope lifetimes so `Global` creation doesn't need raw pointer cast.

- [ ] **render_stream.rs `op_ssr_push_chunk` — document silent drop + TODO for backpressure**
  `try_send` silently drops chunks when channel full. Intentional for v1 (only final
  result matters), but needs documentation and TODO for true streaming wire-up.
