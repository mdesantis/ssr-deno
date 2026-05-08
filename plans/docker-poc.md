# docker-poc

## Problem

The native `.so` statically links V8 — once compiled, it should run without any build deps (Rust, V8 source, LLVM/Clang). A Docker multi-stage build proves this: stage 1 compiles with full toolchain, stage 2 runs with only Ruby + `.so` + pure Ruby files.

This validates the feasibility of shipping prebuilt platform gems (`ssr-deno-x86_64-linux` etc.) where users never need the toolchain.

## Changes

### 1. `Dockerfile` — multi-stage build

**Stage 1 — builder** (`ubuntu:26.04`):

```dockerfile
FROM ubuntu:26.04 AS builder
```

Base deps for V8 + Ruby compilation:

```dockerfile
RUN apt-get install -y build-essential curl git pkg-config ninja-build python3 \
    libglib2.0-dev mold clang-19 lld-19 libclang-19-dev \
    libssl-dev libyaml-dev libreadline-dev libffi-dev zlib1g-dev \
    libgdbm-dev libncurses-dev sccache
```

LLVM/Clang 19 from distro repos (no apt.llvm.org).

Ruby 4.0.3 compiled from source via ruby-build:

```dockerfile
RUN git clone --depth 1 https://github.com/rbenv/ruby-build.git /tmp/ruby-build \
    && /tmp/ruby-build/bin/ruby-build 4.0.3 /usr/local \
    && rm -rf /tmp/ruby-build
```

`git` reinstated in apt-get for the clone.

`gem install bundler`.

Rust via `rustup`. sccache from apt (`sccache` package — precompiled, ~5 min faster than `cargo install`).

```dockerfile
ENV SCCACHE=/usr/bin/sccache
ENV SCCACHE_DIR=/root/.cache/sccache
ENV RUSTFLAGS='-C link-arg=-fuse-ld=mold'
```

`COPY . .` (`.git/` excluded by `.dockerignore`). `minimal-bundle.js` generated inline (bypasses `.dockerignore` excluding `test/`).

```dockerfile
ENV V8_FROM_SOURCE=true
ENV GN_ARGS='v8_monolithic=true v8_monolithic_for_shared_library=true'
ENV LIBCLANG_PATH=/usr/lib/llvm-19/lib
ENV RB_SYS_CARGO_PROFILE=release
```

Single `RUN` with both `bundle install && bundle exec rake compile`, sharing the same BuildKit cache mounts (avoids duplication and handles bundler compiling extensions during install):

```dockerfile
RUN --mount=type=cache,target=/root/.cargo/registry,sharing=locked \
    --mount=type=cache,target=/root/.cargo/git,sharing=locked \
    --mount=type=cache,target=/app/ext/ssr_deno/target,sharing=locked \
    --mount=type=cache,target=/root/.cache/sccache,sharing=locked \
    bundle install && bundle exec rake compile
```

Verify `.so` at `lib/ssr/deno/ssr_deno.so`.

**Stage 2 — runtime** (`ubuntu:26.04`):

Runtime deps only (no `-dev`, no LLVM, no build tools):

```dockerfile
RUN apt-get install -y ca-certificates zlib1g libyaml-0-2 libffi8 \
    libgdbm6 libncurses6
```

`COPY --from=builder /usr/local /usr/local` + `RUN ldconfig` — Ruby 4.0.3.

`COPY --from=builder /app/lib /app/lib` — the compiled `.so` + pure Ruby.

`COPY --from=builder /app/test/fixtures/minimal-bundle.js /app/minimal-bundle.js`.

`COPY docker-entrypoint.rb`.

`ENTRYPOINT ["ruby", "docker-entrypoint.rb"]`.

### 2. `docker-entrypoint.rb` — PoC runner

`$LOAD_PATH` prepend, `require 'ssr/deno'`, `max_heap_size_mb = 64`, `render_timeout_ms = 10_000`, `Bundle.new(minimal-bundle.js).render(name: 'Docker PoC')`. Prints result or exits 1 with backtrace.

### 3. `.dockerignore`

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

Four BuildKit `--mount=type=cache` mounts, shared between `bundle install` and `bundle exec rake compile`:

| Target | Contents | Size |
|--------|----------|------|
| `/root/.cargo/registry` | Downloaded crate sources | ~2 GB |
| `/root/.cargo/git` | Git dependencies | ~500 MB |
| `/app/ext/ssr_deno/target` | Rust artifacts + V8 gn/ninja output | ~15-30 GB |
| `/root/.cache/sccache` | V8 `.o` files keyed by source+flags hash | ~5 GB |

All `sharing=locked` — Cargo and ninja are unsafe under concurrent access.

**Cache management:** `docker buildx prune --filter type=exec.cachemount`. Survives Dockerfile changes (identified by mount target path, not build stage hash).

**Rebuild behavior:**
- Same source: ninja detects nothing changed → instant. Cargo re-links only.
- V8 source changed (submodule update): sccache returns cached `.o` for unchanged files → ninja recompiles only affected `.cc` in ~2-3 minutes.
- Clean builder cache (pruned): full cold build ~90 minutes.

## Not changing

- `extconf.rb`, `Cargo.toml`, `Rakefile`, `ssr-deno.gemspec`
- `vendor/rusty_v8/` or submodule setup
- Platform gem packaging workflow (separate future work)
