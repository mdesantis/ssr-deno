# drain_chunks — sends to closed channel silently

Status: Pending

## Bug

`let _ = chunk_tx.send(chunk).await;` discards the send error.
If the Ruby consumer disconnects early (e.g., `break` in the yield
block), the Rust side keeps calling `send().await` on a closed
channel for every remaining chunk in the drain batch — each call
returns an immediate error, but the loop continues.

## Implementation Draft

In `render_chunked.rs`, change the send loop:

```rust
for chunk in chunks {
    if chunk_tx.send(chunk).await.is_err() {
        // Consumer disconnected (Ruby block raised or was interrupted).
        // Stop sending — the render promise will settle normally and
        // completion/error is communicated via reply_rx in lib.rs.
        break;
    }
}
```

**Risk:** Remaining chunks in the current drain batch are lost, but
the consumer is already gone. The event loop continues until the
promise settles, then the reply channel signals the result.
`#[must_use]` on the render result (via `reply.send(result)`) still
propagates properly.

## Test Strategy

Create a multi-chunk bundle and break early from the block:

```ruby
def with_dual_mode_bundle(count: 5)
  dir = Dir.mktmpdir
  path = File.join(dir, 'dual-mode-bundle.js')
  File.write(path, <<~JS)
    globalThis.render = function() {
      return new Promise(function(resolve) {
        setTimeout(function() {
          if (typeof globalThis.__ssr_push_chunk === 'function') {
            for (var i = 0; i < #{count}; i++) {
              globalThis.__ssr_push_chunk('<chunk>' + i + '</chunk>');
            }
          }
          resolve('<done></done>');
        }, 10);
      });
    };
  JS
  SSR::Deno::Bundle.new(path)
end

def test_render_chunks_after_consumer_disconnect
  bundle = with_dual_mode_bundle(count: 20)

  count = 0
  bundle.render_chunks({}) do |chunk|
    count += 1
    break if count >= 3
  end

  # After break: Rust side got SendError, broke out of loop.
  # Verify a subsequent render works.
  result = bundle.render({})
  assert_includes result, '<done>'
end
```

Without the fix, the `for` loop spins through all 20 chunks calling
`send().await` on a closed channel. With the fix, `break` happens
after the first error. Both cases produce the same end state (no
crash), but the fix eliminates CPU waste.

The test proves non-crash recovery. The CPU waste reduction is
verified by code review.

## Verification

- [x] Implement the fix
- [x] `bundle exec rake` — must exit 0
- [x] Verify `test_render_chunks_after_consumer_disconnect` passes
