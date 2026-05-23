# Plan: JS error message and backtrace extraction

## Context

`RenderError` already exposes `js_error_name`. Adding `js_error_message` and
`js_error_backtrace` completes the API so callers can surface clean JS error
details without parsing `message` themselves.

The core problem: async rejection handlers call `err.toString()`, which drops
the stack. Sync throws already embed a full stack in `message` (via Deno's own
error formatting). Fixing the rejection handler to use `err.stack` instead
unifies both paths — no Rust struct changes, no new globals, no JSON encoding.

## Approach

Two-phase:

1. **Rust/JS (one-liner in 2 files):** Change rejection handlers from
   `err.toString()` to `err.stack || err.toString()` so async rejections carry
   their stack frames into `__ssr_deno_error`. The rest of the poll/propagation
   chain is unchanged.

2. **Ruby-only:** Add `js_error_message` and `js_error_backtrace` to
   `RenderError`, parsing the now-unified message format.

---

## Step 1 — Fix async rejection handlers (Rust/JS)

**Files:** `ext/ssr_deno/src/engine/render.rs:132`,
`ext/ssr_deno/src/engine/render_chunked.rs:65`

Both contain the identical rejection handler. Change:

```js
// before
(err) => {{ globalThis.__ssr_deno_error = (err && err.toString()) || String(err); }}

// after
(err) => {{ globalThis.__ssr_deno_error = (err && err.stack) || (err && err.toString()) || String(err); }}
```

`err.stack` is undefined for non-Error throws — the fallback chain preserves
existing behaviour for those cases.

No change to `cleanup_render_globals`, `poll_render_state`, `RenderState`, or
`SSRDenoError` — all remain as-is.

**Dev-mode coverage confirmed:** `dev_worker.rs:89,106` delegates to the same
`render::render()` and `render_chunked::render_chunked()` functions. No separate
fix needed.

---

## Step 2 — Ruby methods + make RenderError explicit

**File:** `lib/ssr/deno/render_error.rb`

Change `class RenderError` → `class RenderError < Error`. The native extension
(`ssr_deno`) is loaded before `render_error.rb` (`lib/ssr/deno.rb` lines 6-7).
The Rust `init` function (`lib.rs:614`) already defines `RenderError` as a
subclass of `SSR::Deno::Error` via `define_error("RenderError", base_error)`.
Adding `< Error` in Ruby declares explicitly what Rust already set — Ruby
accepts the reopening because the superclass matches.

The full file becomes:

```ruby
module SSR
  module Deno
    class RenderError < Error
      def js_error_name
        message.match(/\b(\w+Error):/i) && ::Regexp.last_match(1)
      end

      def js_error_message
        # \A\S+ matches Rust error_label ("render", "chunked-render")
        msg = message.sub(/\A\S+ failed to start:\s*/i, '')
        msg = msg.sub(/\A\w+Error:\s*/i, '')
        msg.sub(/\n\s+at\s.*\z/m, '')
      end

      def js_error_backtrace
        # NB: false positive possible if error message contains \n    at ...
        m = message.match(/\n((?:\s+at\s.*(?:\n|$))+)/)
        m && m[1].lines.map(&:strip) || nil
      end
    end
  end
end
```

**Why `\A\S+ failed to start:\s*`:**
- Sync throws go through `begin_render` which prefixes the Deno error with
  `"{error_label} failed to start: "` (`render.rs:74`).
- Labels are `"render"` (render.rs) and `"chunked-render"` (render_chunked.rs).
- `\A\S+` matches either; the rest of the pattern is literal.
- Async messages have no such prefix — the first sub is a no-op.

**Unified message format after Step 1:**

| Source | `message` content | `js_error_message` | `js_error_backtrace` |
|--------|-------------------|--------------------|----------------------|
| Sync throw | `"render failed to start: TypeError: msg\n    at ..."` | `"msg"` | `["at file.js:1:2", ...]` |
| Async rejection | `"TypeError: msg\n    at ..."` | `"msg"` | `["at file.js:1:2", ...]` |
| Timeout | `"Render timed out"` | `"Render timed out"` | `nil` |
| Non-Error throw | `"render failed to start: just a string"` | `"just a string"` | `nil` |

---

## Step 3 — RBS

**File:** `sig/ssr/deno.rbs`

Add under `RenderError`:
```rbs
def js_error_message: () -> String
def js_error_backtrace: () -> Array[String]?
```

---

## Step 4 — Tests

**File:** `test/ssr/test_deno_errors.rb`

Add subprocess tests covering the new methods. Naming follows existing
`test_js_error_name_extracts_from_*` convention.

**Timeout tests** use the same mechanism as `test_deno_render_timeout.rb`:
set `render_timeout_ms = 100`, a never-resolving Promise (`new Promise(function() {})`),
then rescue `RenderError` and assert on the exception. Each timeout test
adds ~100ms of real time to the suite.

- `test_js_error_message_extracts_from_sync_throw` — sync TypeError → `"expected number"`
- `test_js_error_message_extracts_from_async_rejection` — async RangeError → `"out of range"`
- `test_js_error_message_returns_raw_for_timeout` — `"Render timed out"` (no stripping)
- `test_js_error_message_extracts_from_non_error_throw` — `throw "raw string"` → `"raw string"`
- `test_js_error_backtrace_returns_frames_for_sync_throw` — returns Array, starts with `"at "`
- `test_js_error_backtrace_returns_frames_for_async_rejection` — returns Array (not nil, after fix)
- `test_js_error_backtrace_returns_nil_for_timeout` — returns `nil`
- `test_js_error_backtrace_returns_nil_for_non_error_throw` — returns `nil`

Unit tests (no subprocess needed):

- `test_js_error_message_returns_raw_for_plain_message` — `RenderError.new("plain")` → `"plain"`
- `test_js_error_backtrace_returns_nil_for_plain_message` — `RenderError.new("plain")` → `nil`

---

## Step 5 — Stale doc audit

- `plans/js-error-message-backtrace.md` — mark implemented; note rejection handler
  change makes async covered; collapse Option A/B distinction
- `CHANGELOG.md` — add entry under `## Unreleased`:
  `RenderError` gains `js_error_message`, `js_error_backtrace`, and explicit
  `< Error` inheritance; async rejections now carry stack frames
- `README.md` — `RenderError` appears in the error table at line 205 but has no
  method docs; add a short section after the table documenting `js_error_name`,
  `js_error_message`, `js_error_backtrace` with example output
- `sig/ssr/deno.rbs` — covered in Step 3
- No `:nocov:` directives in `lib/ssr/deno/render_error.rb` or elsewhere in the
  module — no coverage-annotation adjustments needed
- Re-run existing source-map tests (`test_source_map_resolves_error_location`,
  `test_source_map_disabled_preserves_raw_v8_message`) — switching from
  `err.toString()` to `err.stack` changes message content; `map_render_error`
  runs the source-mapper `resolve()` on the full message string, must verify
  resolution still works

---

## Verification

```sh
bundle exec rake cargo:test          # Rust unit tests pass
bundle exec rake cargo:clippy        # no warnings
bundle exec rake test                # new Ruby tests pass
bundle exec rake coverage:check      # 100% maintained
bundle exec rake                     # full pipeline green
```

Manual smoke: write a test bundle that rejects a Promise with a `TypeError`,
inspect `js_error_message` and `js_error_backtrace` on the raised `RenderError`.

## Critical files

- `ext/ssr_deno/src/engine/render.rs` — rejection handler at line 132
- `ext/ssr_deno/src/engine/render_chunked.rs` — rejection handler at line 65
- `lib/ssr/deno/render_error.rb` — add methods
- `sig/ssr/deno.rbs` — add signatures
- `test/ssr/test_deno_errors.rb` — add tests
- `plans/js-error-message-backtrace.md` — update status
