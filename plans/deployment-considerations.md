# Deployment Considerations

Document deployment considerations for the embedded V8/Deno runtime in production.

Extracted from `rails-integration.md` Phase 3 — see that plan for full context.

---

## Topics

### V8 binary size

- `rusty_v8` produces a shared library (`librusty_v8.so` / `v8.lib`) ~50-100 MB
- Embedded into gem native extension `.so` (~60 MB after compilation)
- Impact on:
  - Deployment artifact size (Docker images, tarballs)
  - Cold start time (loading `.so` into memory)
  - Disk usage per deployment

### Memory

- V8 isolate overhead: ~4-8 MB per isolate (base)
- Per-bundle heap: configurable via `max_heap_size_mb`
- Multiple bundles → multiple isolates → linear memory growth
- V8 may not release memory back to OS (heap fragmentation)
- Memory monitoring: `collect_heap_stats` method

### Docker

- `librusty_v8.so` must be present at runtime
- Multi-stage build: compile in builder stage, copy `.so` to final stage
- Base image: need glibc compatibility (V8 uses glibc)
- Alpine: musl build required (V8 from source with `V8_FROM_SOURCE=true`)

### Platform support

- Linux x86_64: primary target
- macOS: development only
- ARM64: requires `V8_FROM_SOURCE=true` (no prebuilt `rusty_v8` binary)
- Windows: not tested

### CI/CD

- `cargo:test` runs in CI via `bundle exec rake`
- Coverage: 100% line + branch
- Cross-compilation may be needed for ARM64 deployment

---

## Action items

1. Measure `librusty_v8.so` size across platforms
2. Document Docker multi-stage build pattern
3. Document heap sizing guidance per bundle
4. Document platform support matrix
5. Add memory/disk usage note to README

---

## Verification

1. Docker image builds with `ssr_deno` gem
2. SSR works inside Docker container
3. Memory stays within configured heap limit under load
4. Document is reviewed and accurate
