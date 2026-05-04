# Chunked HTTP Streaming SSR (Approach C — Phase 2)

Status: Pending

## Goal

Wire `op_ssr_push_chunk` through to Ruby as a real streaming enumerator so
HTML chunks are flushed to the HTTP response as they arrive, rather than
buffering the full render in memory.

## Prerequisites

- [x] Phase 1: event-loop render with final result (archived)

## Current state

`op_ssr_push_chunk` exists and receives chunks from JS, but `try_send` is
fire-and-forget — chunks are silently dropped. The `mpsc::Receiver` is
created but immediately discarded (`_chunk_rx`). Only the final
`__ssr_stream_result` is returned to Ruby.

## Required changes

### Rust side

- [ ] Change `op_ssr_push_chunk` from `try_send` to `send().await` for backpressure
- [ ] Expose `mpsc::Receiver<String>` to the Ruby caller (via a new native method or wrapped object)
- [ ] Add end-of-stream signal handling (empty chunk or explicit sentinel)
- [ ] Consider timeout per-chunk (not just total render timeout)

### Ruby side

- [ ] New `Bundle#render_stream_chunks` method returning an `Enumerator`
- [ ] Integrate with `ActionController::Live` or Rack `hijack`

### API sketch

```ruby
# Rails controller with ActionController::Live
def show
  include ActionController::Live
  response.headers['Content-Type'] = 'text/html'

  @bundle.render_stream_chunks({ data: @page }).each do |chunk|
    response.stream.write(chunk)
  end
ensure
  response.stream.close
end
```

### Testing

- [ ] Test that chunks arrive incrementally (not all at once)
- [ ] Test backpressure (slow consumer doesn't cause OOM)
- [ ] Test timeout mid-stream
- [ ] Test error propagation during streaming

## Open questions

- Should `render_stream_chunks` coexist with `render_stream`, or replace it?
- What's the chunk granularity — React shell + each Suspense boundary, or finer?
- Should we support Rack 3 streaming (response body as `Enumerator`) directly?
