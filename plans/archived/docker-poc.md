# docker-poc

## Problem

The native `.so` statically links V8 — once compiled, it should run without any build deps (Rust, V8 source, LLVM/Clang). A Docker multi-stage build proves this: stage 1 compiles with full toolchain, stage 2 runs with only Ruby + `.so` + pure Ruby files.

This validates the feasibility of shipping prebuilt platform gems (`ssr-deno-x86_64-linux` etc.) where users never need the toolchain.

## Changes

### 1. `Dockerfile` — multi-stage build

**Stage 1 — builder** (`ubuntu:26.04`):

Base deps for V8 + Ruby compilation (LLVM/Clang 19 from distro, sccache from apt).

Ruby 4.0.3 compiled from source via ruby-build.

Rust via `rustup`.

Cache-optimised layer order:

```dockerfile
# 1. Gem deps (rare changes) → cached separately
COPY Gemfile Gemfile.lock ssr-deno.gemspec ./
COPY lib/ lib/
RUN bundle install

# 2. App source → compile cached separately
COPY . .

ENV GN_ARGS='v8_monolithic=true v8_monolithic_for_shared_library=true'
ENV LIBCLANG_PATH=/usr/lib/llvm-19/lib
ENV RUSTFLAGS='-C link-arg=-fuse-ld=mold'
ENV RUSTC_WRAPPER=sccache
ENV SCCACHE=/usr/bin/sccache
ENV SCCACHE_DIR=/root/.cache/sccache
ENV V8_FROM_SOURCE=true
ENV CARGO_TARGET_DIR=/app/tmp/cargo-target

RUN --mount=type=cache,target=/root/.cargo/registry,sharing=locked \
    --mount=type=cache,target=/root/.cargo/git,sharing=locked \
    --mount=type=cache,target=/app/tmp,sharing=locked \
    --mount=type=cache,target=/root/.cache/sccache,sharing=locked \
    cargo build --manifest-path ext/ssr_deno/Cargo.toml -p ssr_deno --release && \
    cp "$CARGO_TARGET_DIR/release/libssr_deno.so" lib/ssr/deno/ssr_deno.so
```

`RB_SYS_CARGO_PROFILE` removed — raw `cargo build --release` replaces `bundle exec rake compile`.

`hmr` feature on `deno_runtime` enables `include_js_files_for_snapshotting` → deno_core stores `CARGO_MANIFEST_DIR` paths in binary instead of embedding JS. A subsequent RUN copies deno extension JS/TS sources from cargo registry cache to `/app/deno-ext-src/`, preserving relative path under `/root/.cargo/`.

**Stage 2 — runtime** (`ubuntu:26.04`):

Runtime deps only (no `-dev`, no LLVM, no build tools).

`COPY --from=builder /usr/local /usr/local` + `RUN ldconfig` — Ruby 4.0.3.

Deno extension JS/TS sources at build-time paths:
```dockerfile
COPY --from=builder /app/deno-ext-src/ /root/.cargo/
```

Gem source staged (no V8 bloat):
```dockerfile
COPY --from=builder /app/lib /ssr-deno/lib
COPY --from=builder /app/ext/ssr_deno/extconf.rb /ssr-deno/ext/ssr_deno/extconf.rb
COPY --from=builder /app/ssr-deno.gemspec /ssr-deno/
```

`/poc` app with Gemfile path source:
```dockerfile
RUN mkdir -p /poc && printf '%s\n' \
    'source "https://rubygems.org"' '' \
    'gem "ssr-deno", path: "/ssr-deno"' > /poc/Gemfile && \
    printf '%s\n' \
    '#!/usr/bin/env ruby' '# frozen_string_literal: true' '' \
    'require "bundler/setup"' 'require "ssr/deno"' '' \
    'SSR::Deno.max_heap_size_mb = 64' \
    'SSR::Deno.render_timeout_ms = 10_000' '' \
    'begin' \
    '  bundle = SSR::Deno::Bundle.new("/ssr-deno/minimal-bundle.js")' \
    '  result = bundle.render({ data: { name: "Docker PoC" } })' \
    '  puts "SSR result: #{result}"' \
    '  puts "OK: gem works via path source."' \
    'rescue StandardError => error' \
    '  warn "FAIL: #{error.class}: #{error.message}"' \
    '  warn error.backtrace.first(3).join("\n")' \
    '  exit 1' \
    'end' > /poc/app.rb

WORKDIR /poc
RUN bundle install
ENTRYPOINT ["ruby", "app.rb"]
```

### 2. `docker-entrypoint.rb` — removed

Inlined as `app.rb` in Dockerfile. No separate file.

### 3. `ssr-deno.gemspec`

Added `'ssr-deno.gemspec'` to `spec.files`.

### 4. `.dockerignore`

Kept as-is. Inline bundle generation in Dockerfile avoids modifying it.

## Build commands

```
docker build -t ssr-deno-poc .                           # PoC (stage 2)
docker build -t ssr-deno-builder --target builder .      # base image for apps
```

## Runtime deps verification

`ldd` on both `.so` and Ruby binary confirms only these NEEDED:

| Library | Package | Required by |
|---------|---------|-------------|
| `libruby.so.4.0` | copied from builder (`/usr/local/lib`) | Ruby + `.so` |
| `libz.so.1` | `zlib1g` | Ruby + `.so` |
| `libgcc_s.so.1` | built-in (gcc runtime) | `.so` |
| `libm.so.6` | built-in (glibc) | Ruby + `.so` |
| `libc.so.6` | built-in (glibc) | Ruby + `.so` |

`libstdc++` is NOT in NEEDED (statically linked via `v8_monolithic=true` and Rust's cdylib default).

## Cache strategy

Five BuildKit `--mount=type=cache` mounts:

| Target | Contents | Size |
|--------|----------|------|
| `/root/.cargo/registry` | Downloaded crate sources | ~2 GB |
| `/root/.cargo/git` | Git dependencies | ~500 MB |
| `/app/tmp` | Rust artifacts + V8 gn/ninja output | ~15-30 GB |
| `/root/.cache/sccache` | V8 `.o` files keyed by source+flags hash | ~5 GB |

All `sharing=locked` — Cargo and ninja are unsafe under concurrent access.

**Cache management:** `docker buildx prune --filter type=exec.cachemount`. Survives Dockerfile changes (identified by mount target path, not build stage hash).

**Rebuild behavior:**
- Gemfile change → layer 1 (bundle install) + compile invalidated (~1min)
- Ruby/docs change → compile only, cargo relink (~15-25s)
- Same source: ninja detects nothing changed → instant. Cargo re-links only.
- V8 source changed (submodule update): sccache returns cached `.o` for unchanged files → ninja recompiles only affected `.cc` in ~2-3 minutes.
- Clean builder cache (pruned): full cold build ~90 minutes.

## Notable issues

- `hmr` feature → `include_js_files_for_snapshotting` → deno_core stores CARGO_MANIFEST_DIR in binary. JS/TS files must exist at those exact paths at runtime. Fixed by copying cargo registry source dirs in builder.
- `sccache` with `RUSTC_WRAPPER` reduces rebuild time for Rust changes.

## Not changing

- `extconf.rb`, `Cargo.toml`, `Rakefile` (except `hmr` feature)
- `vendor/rusty_v8/` or submodule setup
- Platform gem packaging workflow (separate future work)
