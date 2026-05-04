# Plan: Remove "stream" nomenclature from internals

**Status:** Complete
**Goal:** Reduce "stream" leakage from internal code. The concept is user-facing (SSR streaming); internals should use domain-accurate names.

## Renames

| Category | Current | Proposed |
|----------|---------|----------|
| Ruby public API | `Bundle#render_stream_chunks` | `Bundle#render_chunks` |
| Ruby native bridge | `SSR::Deno.native_render_stream_chunks` | `SSR::Deno.native_render_chunks` |
| Rust magnus binding | `native_render_stream_chunks` | `native_render_chunks` |
| JS globals | `__ssr_stream_result`, `__ssr_stream_error`, `__SSR_STREAM_SENTINEL` | `__ssr_deno_result`, `__ssr_deno_error`, `__SSR_DENO_SENTINEL` |
| Extension name | `"ssr_stream"` | `"ssr_deno_ops"` |
| Test file | `test_deno_render_stream.rb` | `test_deno_render.rb` |
| Test file | `test_deno_render_stream_chunks.rb` | `test_deno_render_chunks.rb` |

## Implementation checklist

- [x] Rename JS globals in `render.rs` and `render_chunked.rs`
- [x] Rename extension `"ssr_stream"` → `"ssr_deno_ops"` in `mod.rs`
- [x] Rename `native_render_stream_chunks` → `native_render_chunks` in `lib.rs`
- [x] Rename `render_stream_chunks` → `render_chunks` in `bundle.rb`
- [x] Update `sig/ssr/deno.rbs`
- [x] Rename test files (`git mv`) and update their contents
- [x] Update comments in Rust files (remove "streaming" where inaccurate)
- [x] Update `README.md` — method name, section title
- [x] Update `CHANGELOG.md` — method name references
- [x] Update `plans/always-on-event-loop.md` — references
- [x] Update `docs/architecture.md` — render_chunked.rs description
- [x] Run `bundle exec rake` — must exit 0

## What stays as "stream"

- `node:stream` (Node.js module name)
- Archived plans in `plans/archived/` (historical record)
- User-facing prose describing the *concept* of "streaming SSR" (domain term)
- Sample directory names like `vite-react-streaming-ssr-app`
- `response.stream.write` in README examples (Rails API)

## Dependencies

- None — pure rename refactoring.
