# JS Error message and backtrace extraction

Status: design complete — see [js-error-message-backtrace-impl.md](js-error-message-backtrace-impl.md) for implementation plan

## Current state

`RenderError` has `js_error_name` (via `render_error.rb`), extracting JS error class
name from `message` using regex `/\b(\w+Error):/i && Regexp.last_match(1)`.

Rust wraps sync throws with a prefix before they reach Ruby (`render.rs:74`):

```rust
Err(SSRDenoError::Render(format!("{error_label} failed to start: {msg}")))
```

So `RenderError#message` format depends on source:

| Source | `self.message` content | Stack info |
|--------|----------------------|------------|
| Sync `throw new Error("msg")` | `"render failed to start: TypeError: msg\n    at file:1:2\n..."` | embedded in message, after Rust prefix |
| Async rejection (`err.toString()`) | `"TypeError: msg"` | **missing** — `.toString()` drops stack |
| Timeout | `"Render timed out"` | none |
| Non-Error throw | `"render failed to start: just a string"` | none |

## Goal

Add `js_error_message` and `js_error_backtrace` to `RenderError`.

## Option A: Ruby-only (approved for js_error_message)

### `js_error_message`

Strip the Rust prefix and `ClassName:` prefix, then drop the `\n    at ...` suffix.
Must handle sync throws where the message has the `"render failed to start: "` wrapper.

```ruby
def js_error_message
  msg = message.sub(/\Arender(?:\s+\w+)*:\s*/, '')
  msg = msg.sub(/\A\w+Error:\s*/, '')
  msg.sub(/\n\s+at\s.*\z/m, '')
end
```

| Input (`message`) | Output |
|---|---|
| `"TypeError: expected number"` | `"expected number"` |
| `"render failed to start: TypeError: msg\n    at file.js:1:2"` | `"msg"` |
| `"render failed to start: Error: boom\n    at file.js:1:2"` | `"boom"` |
| `"Render timed out"` | `"Render timed out"` |
| `"render failed to start: just a string"` | `"just a string"` |

> Note: the Rust prefix (`"render failed to start: "`, `"render timed out"`) is
> implementation detail that leaks into `message`. The prefix pattern may evolve —
> keep `js_error_message` in sync if `error_label` strings change in `render.rs`.

### `js_error_backtrace`

Extract `\n    at ...` lines from message. Async rejections return `nil`
(no stack in `.toString()`).

```ruby
def js_error_backtrace
  m = message.match(/\n((?:\s+at\s.*(?:\n|$))+)/)
  m && m[1].lines.map(&:strip) || nil
end
```

### Completion checklist for Option A

- [ ] `render_error.rb` — add `js_error_message`, `js_error_backtrace`
- [ ] `sig/ssr/deno.rbs` — add signatures for both methods
- [ ] `test_deno_errors.rb` — add tests:
  - `js_error_message` for sync throw, async rejection, timeout, non-Error throw
  - `js_error_backtrace` returns frames for sync throw, nil for async rejection

## Option B: Rust-backed full backtrace

Capture `err.stack` in the JS rejection handler so async rejections also carry
stack info. **Three** files need updating: `render.rs`, `render_chunked.rs`, and
`dev_load.rs` (dev mode has its own rejection handler).

JS side (in all three files):
```javascript
(err) => {
  globalThis.__ssr_deno_error = (err && err.toString()) || String(err);
  globalThis.__ssr_deno_error_stack = (err && err.stack) || null;
}
```

Rust:
- `poll_render_state` protocol must change — currently returns a single tagged
  string (`"E:<msg>"` or `"R:<result>"`). To carry stack too, JSON-encode both,
  or do a second `execute_script` call only when error is set.
- `RenderState::Error(String)` → `RenderState::Error(String, Option<String>)`
  (breaks all match arms).
- `cleanup_render_globals` must also clear `__ssr_deno_error_stack` or it leaks
  across renders.
- `map_render_error` sets the stack on the Ruby `RenderError`.

Stack availability:

| Source | Option A | Option B |
|--------|----------|----------|
| Sync `throw` | ✅ parsed from message | ✅ from `err.stack` |
| Async rejection | ❌ nil | ✅ from `err.stack` |
| Timeout | ❌ nil | ❌ nil |
| Non-Error throw | ❌ nil | ❌ nil |

## Variant B/C: set_backtrace instead of accessor

Instead of a `js_error_backtrace` accessor, call `error.set_backtrace(lines)` in
Rust after constructing the `RenderError`. This integrates with Ruby's standard
`$!` / `e.backtrace` / logger output.

Pros:
- Shows JS frames in logs when reporting exceptions
- Works with any tool that reads `Exception#backtrace`

Cons:
- Pollutes the Ruby backtrace concept (JS frame formatting differs from Ruby)
- `set_backtrace` replaces the full backtrace — Ruby FFI call site is lost unless
  explicitly merged (append JS frames after Ruby frames, but looks wrong in tools)

## Variant B/D: cause

Create a separate `StandardError` as the `cause` of `RenderError`, carrying the
JS message and stack. The Ruby exception chain becomes:

```
RenderError ("render failed to start: TypeError: expected number")
  └─ cause: StandardError ("expected number")
       └─ backtrace: ["at file.js:1:2", ...]
```

Pros:
- Clean separation of Ruby and JS concerns
- `$!.cause` available in debuggers and loggers
- No backtrace pollution on the outer error

Cons:
- Not all loggers surface `cause`
- Setting `cause` from magnus is non-trivial: Ruby sets it automatically only
  when an exception is raised inside a rescue block. From Rust/FFI, requires
  either a Ruby eval or `Ruby::protect` scaffolding.

## Open questions

1. Is `js_error_backtrace` accessor enough, or should we integrate deeper?
2. Is the Rust complexity worth it for async stack coverage?
3. Variant C or D preferred if Rust path chosen?
