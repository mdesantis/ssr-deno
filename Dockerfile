# hadolint global ignore=DL3008
# Build:
#   docker build -t ssr-deno-poc --target poc .           # PoC demo
#   docker build -t ssr-deno-base --target base .          # reusable base for apps
#   docker build -t ssr-deno-builder --target builder .    # compile-only

ARG RUBY_VERSION=4.0.5

# Stage 1: Build the native extension + Ruby
FROM ubuntu:resolute AS builder

ARG RUBY_VERSION

SHELL ["/bin/bash", "-o", "pipefail", "-c"]

# Build toolchain for Ruby + V8
RUN apt-get update -qq && apt-get install -y --no-install-recommends \
    build-essential \
    ca-certificates \
    curl \
    git \
    pkg-config \
    ninja-build \
    python3 \
    libglib2.0-dev \
    mold \
    clang-21 \
    lld-21 \
    libclang-21-dev \
    libssl-dev \
    libyaml-dev \
    libreadline-dev \
    libffi-dev \
    zlib1g-dev \
    libgdbm-dev \
    libncurses-dev \
    sccache \
    && rm -rf /var/lib/apt/lists/*

# Compile Ruby from source via ruby-build
RUN git clone --depth 1 https://github.com/rbenv/ruby-build.git /tmp/ruby-build \
    && /tmp/ruby-build/bin/ruby-build $RUBY_VERSION /usr/local \
    && rm -rf /tmp/ruby-build

# Install Rust
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --no-modify-path
ENV PATH=/root/.cargo/bin:$PATH

WORKDIR /app

# Cache-optimised layer order:
#   1. Gem deps (rare changes) → bundle install cached separately
#   2. Rust sources + V8 vendor → cargo build cached separately
#   3. Everything else → fast layers, Rust stays cached
# Gemfile change = only layer 1 + final layers invalidated
# Ruby/docs change = only layer 3 invalidated (no Rust rebuild!)
# Rust change = layers 2 + 3 invalidated

COPY Gemfile Gemfile.lock ssr-deno.gemspec ./
COPY lib/ lib/
RUN bundle install

# Rust compilation deps only — changes here trigger cargo rebuild
COPY ext/ssr_deno/Cargo.toml ext/ssr_deno/Cargo.lock ext/ssr_deno/
COPY ext/ssr_deno/src/ ext/ssr_deno/src/
COPY ext/ssr_deno/crates/ ext/ssr_deno/crates/
COPY vendor/ vendor/
RUN sed -i '/inputs = rustc_wrapper_inputs/d' \
    vendor/rusty_v8/build/toolchain/gcc_toolchain.gni

ENV GN_ARGS='v8_monolithic=true v8_monolithic_for_shared_library=true'
ENV LIBCLANG_PATH=/usr/lib/llvm-21/lib
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
    cp "$CARGO_TARGET_DIR/release/libssr_deno.so" /tmp/libssr_deno.so

# Copy rest of the project (Ruby, config, tests, docs).
# Layer invalidates on non-Rust changes — cargo stays cached.
COPY . .

# Restore the .so built in the Rust layer (COPY . . overwrote it)
RUN cp /tmp/libssr_deno.so lib/ssr/deno/ssr_deno.so && rm /tmp/libssr_deno.so

# Copy Deno extension JS/TS sources for runtime.
# When hmr → include_js_files_for_snapshotting → deno_core stores
# compile-time paths (CARGO_MANIFEST_DIR) in binary. At runtime these
# files must exist at the same absolute path.
RUN --mount=type=cache,target=/root/.cargo/registry,sharing=locked \
    dest=/app/deno-ext-src; \
    mkdir -p "$dest"; \
    for crate_dir in /root/.cargo/registry/src/*/deno_*/; do \
        if [ -d "$crate_dir" ] && find "$crate_dir" -maxdepth 4 \( -name "*.js" -o -name "*.ts" \) -print -quit 2>/dev/null | grep -q .; then \
            rel="${crate_dir#/root/.cargo/}"; \
            mkdir -p "$dest/$(dirname "$rel")"; \
            cp -a "$crate_dir" "$dest/$(dirname "$rel")/"; \
        fi; \
    done

# Stage 2: Base runtime (Ruby + .so + JS deps, no app — for FROM in other projects)
FROM ubuntu:resolute AS base

SHELL ["/bin/bash", "-o", "pipefail", "-c"]

# Runtime deps for Ruby binary (no build toolchain)
RUN apt-get update -qq && apt-get install -y --no-install-recommends \
    ca-certificates \
    zlib1g \
    libyaml-0-2 \
    libffi8 \
    libgdbm6 \
    libncurses6 \
    && rm -rf /var/lib/apt/lists/*

# Copy compiled Ruby from builder
COPY --from=builder /usr/local /usr/local
RUN ldconfig

# Deno extension JS/TS sources at build-time paths.
# The native extension has these paths baked in at compile time. We copy the
# files to a neutral location and symlink from /root/.cargo so non-root users
# can access them without opening /root/ entirely to the world.
COPY --from=builder /app/deno-ext-src/ /var/lib/deno-cargo/
RUN rm -rf /root/.cargo 2>/dev/null; \
    ln -sf /var/lib/deno-cargo /root/.cargo && \
    chmod -R a+rX /var/lib/deno-cargo/ && \
    chmod a+x /root/

# Stage gem source (no V8 bloat)
COPY --from=builder /app/lib /ssr-deno/lib
COPY --from=builder /app/ext/ssr_deno/extconf.rb /ssr-deno/ext/ssr_deno/extconf.rb
COPY --from=builder /app/ssr-deno.gemspec /ssr-deno/

# Generate minimal JS bundle for quick smoke-test
RUN printf '%s\n' \
    'globalThis.render = function(data) {' \
    '  var parsed = typeof data === "string" ? JSON.parse(data) : data;' \
    '  var name = (parsed.data && parsed.data.name) || "world";' \
    '  return "<h1>" + name + "</h1>";' \
    '}' > /ssr-deno/minimal-bundle.js

# Stage 3: PoC demo (extends base with a test app)
FROM base AS poc

# Create PoC app with Gemfile path source
RUN mkdir -p /poc && \
    printf '%s\n' \
      'source "https://rubygems.org"' \
      '' \
      'gem "ssr-deno", path: "/ssr-deno"' \
    > /poc/Gemfile && \
    printf '%s\n' \
      '#!/usr/bin/env ruby' \
      '# frozen_string_literal: true' \
      '' \
      'require "bundler/setup"' \
      'require "ssr/deno"' \
      '' \
      'SSR::Deno::Config.max_heap_size_mb = 64' \
      'SSR::Deno::Config.render_timeout_ms = 10_000' \
      '' \
      'begin' \
      '  bundle = SSR::Deno::Bundle.new("/ssr-deno/minimal-bundle.js")' \
      '  result = bundle.render({ data: { name: "Docker PoC" } })' \
      '  puts "SSR result: #{result}"' \
      '  puts "OK: gem works via path source."' \
      'rescue StandardError => error' \
      '  warn "FAIL: #{error.class}: #{error.message}"' \
      '  warn error.backtrace.first(3).join("\n")' \
      '  exit 1' \
      'end' \
    > /poc/app.rb

WORKDIR /poc
RUN bundle install

ENTRYPOINT ["ruby", "app.rb"]
