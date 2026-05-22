# JS Error message and backtrace extraction

Status: design discussion

## Current state

`RenderError` has `js_error_name` (via `render_error.rb`), extracting JS error class
name from `message` using regex `/\\b(\\w+Error):/i && Regexp.last_match(1)`.

Error message format depends on source:

| Source | `self.message` content | Stack info |
|--------|----------------------|------------|
| Sync `throw new Error("msg")` | `"TypeName: msg\\n    at file:1:2\\n..."` | embedded in message |
| Async rejection (`err.toString()`) | `"TypeName: msg"` | **missing** — `.toString()` drops stack |
| Timeout | `"did not settle within ..."` | none |
| Non-Error throw | `"just a string"` | none |

## Goal

Add `js_error_message` and `js_error_backtrace` to `RenderError`.

## Option A: Ruby-only (approved for js_error_message)

### `js_error_message`

Strip `ClassName: ` prefix and `\\n    at ...` suffix:

```ruby
def js_error_message
  msg = message.sub(/\A\w+Error:\s*/, '')
  msg.sub(/\n\s+at\s.*\z/m, '')
end
```

| Input | Output |
|---|---|
| `"TypeError: expected number"` | `"expected number"` |
| `"Error: boom\\n    at file.js:1:2"` | `"boom"` |
| `"did not settle"` | `"did not settle"` |
| `"just a string"` | `"just a string"` |

### `js_error_backtrace`

Extract `\\n    at ...` lines from message. Async rejections return `nil`
(no stack in `.toString()`).

```ruby
def js_error_backtrace
  m = message.match(/\n((?:\s+at\s.*(?:\n|$))+)/)
  m && m[1].lines.map(&:strip) || nil
end
```

## Option B: Rust-backed full backtrace

Capture `err.stack` in the JS rejection handler so async rejections also carry
stack info. Requires changes in `render.rs` and `render_chunked.rs`.

JS side:
```javascript
(err) => {
  globalThis.__ssr_deno_error = (err && err.toString()) || String(err);
  globalThis.__ssr_deno_error_stack = (err && err.stack) || null;
}
```

Rust: `poll_render_state` reads `__ssr_deno_error_stack`, extends
`RenderState::Error(String)` to carry both message and stack.
`map_render_error` sets the stack on the Ruby `RenderError`.

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
- Need to preserve the Ruby origin (FFI call site) — merge or append?

## Variant B/D: cause

Create a separate `StandardError` as the `cause` of `RenderError`, carrying the
JS message and stack. The Ruby exception chain becomes:

```
RenderError ("TypeError: expected number")
  └─ cause: StandardError ("expected number")
       └─ backtrace: ["at file.js:1:2", ...]
```

Pros:
- Clean separation of Ruby and JS concerns
- `$!.cause` available in debuggers and loggers
- No backtrace pollution

Cons:
- Not all loggers surface `cause`

## Open questions

1. Is `js_error_backtrace` accessor enough, or should we integrate deeper?
2. Is the Rust complexity worth it for async stack coverage?
3. Variant C or D preferred if Rust path chosen?
