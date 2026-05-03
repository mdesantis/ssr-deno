# SSR Stability Tests

## Problem

The codebase has no tests that catch regressions in three categories:

| Category | Risk | Existing coverage |
|---|---|---|
| **Memory leaks** (ours) | V8 heap grows unboundedly across renders | None — no repeated-render + heap-snapshot tests |
| **Segfaults** | Rust/magnus/V8 boundary crashes under stress | None — no stress tests with large data, rapid reloads, or saturation |
| **Edge-case data** | Malformed/null/deeply-nested JSON input causes V8 panic or Rust panic | None — only happy-path payloads tested |

The concern is that a refactoring in the Rust FFI layer or V8 scope chain handling could introduce a leak or crash that only manifests after hundreds of renders — invisible to the current test suite.

## What's feasible vs. not

### Feasible

- **Repeated-render leak detection** — render N times, snapshot `used_heap_size` before/after. Use a generous ratio threshold (≤ 3x growth) to tolerate V8 GC non-determinism. A genuine leak would show 10x+ growth.
- **Large data payload** — 100 KB+ JSON input. Verifies JSON deserialization memory and V8 string handling don't segfault.
- **Edge-case data** — `{}`, `null`, deeply nested objects. Verifies no V8 panic or Rust panic at the V8/Rust boundary.
- **Rapid reload** — reload a bundle multiple times between renders. Verifies `Box::leak` path, namespace swap atomicity, and no stale function references.
- **Pool saturation** — already covered by `test_deno_concurrency.rb` (20 threads, pool_size=1). Not duplicated in this file.

### Not feasible (or too risky)

| Concern | Why not |
|---|---|
| **Intentional V8 OOM** | `max_heap_size_mb` is `max_old_generation_size_in_bytes`. When V8 hits it, V8 aborts the process — the test itself segfaults. Testable only via subprocess with `assert exit != 0`, but the signal is V8's own behavior, not ours. |
| **Worker death recovery** | Pool is `OnceLock` — no public API to tear down and re-init. Already noted as skipped test. |
| **Absolute heap equality** | V8 GC is non-deterministic — heap doesn't shrink on demand. Must use ratio threshold, not exact match. |
| **User leak graceful handling** | Same as V8 OOM above — pushing a user component to allocate past `max_heap_size_mb` aborts the process. Not testable safely in-process. |

## Implementation Steps

### [x] Step 1: Add test fixtures

**File:** `test/fixtures/large-payload-bundle.js` (created)

### [x] Step 2: Add stability test file

**File:** `test/ssr/test_deno_stability.rb` (created)

4 tests total. Concurrent stress and pool saturation are already covered by `test_deno_concurrency.rb` (20 threads, pool_size=1). Not duplicated.

Test A — repeated-render leak detection:
```ruby
def test_no_internal_leaks_over_repeated_renders
  baseline = SSR::Deno.heap_stats['used_heap_size']
  100.times { @bundle.render({ data: { name: 'stress' } }) }
  final = SSR::Deno.heap_stats['used_heap_size']
  assert_operator final, :<, baseline * 3,
                  "Heap grew #{final / baseline}x — possible leak"
end
```

Test B — large data payload:
```ruby
def test_large_data_payload_does_not_crash
  large = { items: Array.new(1000) { { name: 'x' * 80, value: rand } } }
  result = @bundle.render({ data: large })
  assert_match(%r{<div>}, result)
end
```

Test C — edge-case data:
```ruby
def test_edge_case_data_does_not_crash
  @bundle.render({})
  @bundle.render({ data: nil })
  @bundle.render({ data: { deep: { deeper: { deepest: [1, 2, 3] } } } })
end
```

Test D — rapid reload:
```ruby
def test_rapid_reload_does_not_crash
  bundle_path = File.expand_path('../fixtures/minimal-bundle.js', __dir__)
  20.times do
    bundle = SSR::Deno::Bundle.new(bundle_path)
    3.times { bundle.reload; bundle.render({ data: { name: 'reload' } }) }
  end
end
```

### [x] Step 3: Integrate into test runner

No changes needed — `test_deno_stability.rb` matches the `test_*.rb` pattern and is automatically picked up by `test:main` (the `test.rake` glob excludes only specific files by name, not by pattern).

### [x] Step 4: Verify

`bundle exec rake` passes — Rust compile, cargo test, sample builds, all Ruby test suites (including stability), RuboCop, SimpleCov 100%.

## Files Changed

| File | Change |
|---|---|
| `test/fixtures/large-payload-bundle.js` | New fixture — renders + stringifies full payload |
| `test/ssr/test_deno_stability.rb` | New test file — 4 stability tests |

## Files NOT Changed

| File | Reason |
|---|---|
| `ext/ssr_deno/src/` | No Rust changes |
| `lib/ssr/deno.rb` | No API changes |
| `sig/ssr/deno.rbs` | No type changes |
| `rakelib/test.rake` | test:main glob auto-discovers `test_deno_stability.rb` |
| `README.md` / `docs/architecture.md` | Implementation detail, not architectural |

## Verification

- `bundle exec rake` exits 0
- Stability tests pass — 0 failures, 0 errors
- RuboCop clean on new test file
- SimpleCov line + branch 100% (stability tests add no new code paths in `lib/`, only exercise existing ones)
