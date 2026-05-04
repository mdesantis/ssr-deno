# Rust test coverage gaps — `ssr_deno_core`

Status: ✅ Complete

## Context

`cargo test -p ssr_deno_core` covers pure-Rust types/functions without V8.
Audit identified missing `Display` tests and weak error-message assertions.

## Findings

| Gap | Severity | Fix |
|-----|----------|-----|
| `SSRDenoError::OutOfMemory` Display untested | High | Added test |
| `SSRDenoError::HeapStatsSerialization` Display untested | High | Added test |
| `validate_render_timeout_ms` error messages not asserted | Medium | Strengthened existing tests |
| `Config` Clone/Copy traits | Low | Derived — skip |
| `resolve_pool_size` when parallelism=1 | Low | Not testable without mocking — skip |

## Implementation checklist

- [x] Add `deno_error_display_out_of_memory` test
- [x] Add `deno_error_display_heap_stats_serialization` test
- [x] Assert error message content in `validate_render_timeout_rejects_99`
- [x] Assert error message content in `validate_render_timeout_rejects_300001`
- [x] `bundle exec rake` passes (33 Rust tests, all Ruby suites, 100% coverage)
