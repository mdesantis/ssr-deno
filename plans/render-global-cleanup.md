# render.rs — missing JS global cleanup after buffered render

Status: Pending

## Bug

The buffered render (`render.rs`) sets `__ssr_deno_result` and
`__ssr_deno_error` on `globalThis` but never cleans them up.
`render_chunked.rs` cleans its globals (`__ssr_chunks`,
`__ssr_push_chunk`) explicitly — `render.rs` does not. Reset on
next render masks the leak, but it wastes global object slots
and is inconsistent.

The `execute_script` error path in both `render.rs` and
`render_chunked.rs` also misses cleanup (the startup script DID
set globals before the throw).

## Implementation Draft

**Fix:** Add a shared cleanup helper and call it from both render
flavours.

Add to `render.rs`:

```rust
/// Cleans up render-state globals to prevent leakage across renders.
fn cleanup_render_globals(worker: &mut MainWorker) {
    let _ = worker.execute_script(
        "<ssr-deno:render-cleanup>",
        "globalThis.__ssr_deno_result = undefined; \
         globalThis.__ssr_deno_error = undefined;"
            .to_string()
            .into(),
    );
}
```

In `render::render()` — add cleanup in two places:

1. **Early-error path** (after `execute_script` failure, before each `return`):

   ```rust
   if let Err(e) = exec_result {
       watchdog.cancel();
       cleanup_render_globals(worker);
       // ...existing OOM/timeout/error dispatch...
   }
   ```

2. **Normal path** (after watchdog cancel, before returning `result`):

   ```rust
   watchdog.cancel();

   if timeout_triggered.load(Ordering::SeqCst) || oom_triggered.load(Ordering::SeqCst) {
       worker.js_runtime.v8_isolate().cancel_terminate_execution();
   }

   cleanup_render_globals(worker);

   result
   ```

In `render_chunked.rs` — same treatment for the early-error path:

```rust
if let Err(e) = exec_result {
    watchdog.cancel();
    cleanup_render_globals(worker);
    // ...existing OOM/timeout/error dispatch...
}
```

The normal-path cleanup in `render_chunked.rs` already uses its own
script (sets chunk-specific globals). Keep that as-is.

**Risk:** `cancel_terminate_execution()` must be called before
`execute_script` for cleanup, since a terminated isolate rejects
further script execution. The fix preserves this order (cleanup
comes after the `if` block).

## Test Strategy

The cleanup is cosmetic/correctness, not functional. The startup
script always overwrites `__ssr_deno_result` / `__ssr_deno_error`
at the top of every render call, so the fix has no observable
behavior change from Ruby. Existing multi-render tests
(test_deno_render.rb, test_deno_concurrency.rb) implicitly verify
no regression.

Investigation note: the test written for this fix
(`test_render_after_failed_execute_script`) passes without the
Rust changes because the startup script re-initialises the globals
before the next render function runs. The fix is still valid for
cosmetic consistency with `render_chunked`, but cannot be tested
as red-green from Ruby. Confirmed by code review.

**Test — error-path cleanup via invalid render:**

```ruby
def test_render_after_failed_render_still_works
  bundle = ::SSR::Deno::Bundle.new(MINIMAL_BUNDLE)

  # Trigger an error that enters the execute_script error path
  # and exercises the early-return cleanup.
  assert_raises(::SSR::Deno::RenderError) do
    bundle.render('!invalid-json', raw_input: true)
  end

  # Second render should succeed — cleanup from the first attempt
  # shouldn't interfere with the next startup script.
  result = bundle.render({ data: { name: 'recovery' } })
  assert_includes result, 'recovery'
end
```

**Test — normal-path cleanup via consecutive renders:**
Already covered by existing tests. Not adding a new one.

## Verification

- [ ] Implement the fix
- [ ] `bundle exec rake` — must exit 0
- [ ] Verify `test_render_after_failed_render_still_works` passes
