# poll_render_state — corrupt sentinel edge case

Status: Pending

## Bug

The `else` branch in `poll_render_state` returns `RenderState::Pending`
for any unrecognised string returned by the JS poll expression. If
`__ssr_deno_result` or `__ssr_deno_error` enter an unexpected state,
the event loop spins until the watchdog fires with a generic "timed
out" message instead of surfacing the real problem.

## Analysis

The JS poll expression:
```js
globalThis.__ssr_deno_error
 ? ('E:' + globalThis.__ssr_deno_error)
 : (globalThis.__ssr_deno_result === globalThis.__SSR_DENO_SENTINEL
    ? null
    : ('R:' + JSON.stringify(globalThis.__ssr_deno_result)))
```

This can ONLY return: `null`, a string starting with `"E:"`, or a
string starting with `"R:"`. A corrupt sentinel
(`__SSR_DENO_SENTINEL` changed to a non-object) causes the poll
to misidentify `__ssr_deno_result` as a real result and produce
`'R:' + JSON.stringify({})` — which still starts with `R:` and is
handled correctly.

The `else` branch is therefore unreachable with the current JS
expression. The fix is pure defense-in-depth for future changes.

## Implementation Draft

In `render.rs`, change the `else` branch:

```rust
} else {
    // Unrecognised format — the poll JS returned something unexpected.
    // Surface this as an error to avoid infinite polling until timeout.
    RenderState::Error(format!(
        "Render state poll returned unrecognised value (prefix: {})",
        s.chars().take(20).collect::<String>()
    ))
}
```

The 20-char truncation prevents leaking large result strings into
error messages while giving enough context.

**Risk:** Theoretical false positive — a render result that starts
with neither `E:` nor `R:`. No real result can trigger this since
`JSON.stringify` always produces a valid JSON value, none of which
start with `E` or `R` when prefixed with `R:`. The risk is zero.

## Test Strategy

The defense-in-depth else-branch is not directly testable from Ruby
(the JS expression always produces known prefixes). Verified by code
review and the following sentinel-corruption test that proves no
regression:

```ruby
def test_render_with_corrupted_sentinel_still_returns
  dir = Dir.mktmpdir
  path = File.join(dir, 'corrupt-sentinel.js')
  File.write(path, <<~JS)
    globalThis.render = function() {
      globalThis.__SSR_DENO_SENTINEL = 42;
      return '<html/>';
    };
  JS
  bundle = SSR::Deno::Bundle.new(path)

  # Corruption causes poll to see old sentinel {} as result,
  # but the render still completes (doesn't hang).
  result = bundle.render({})
  assert_kind_of String, result
end
```

## Verification

- [x] Implement the fix
- [x] `bundle exec rake` — must exit 0
- [x] Verify `test_render_with_corrupted_sentinel_still_returns` passes
