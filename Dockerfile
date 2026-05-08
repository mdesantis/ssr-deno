# Build:   docker build -t ssr-deno-poc .                          # PoC (stage 2)
# Base:    docker build -t ssr-deno-builder --target builder .    # for FROM in apps

# Stage 1: Build the native extension + Ruby 4.0.3
FROM ubuntu:26.04 AS builder

SHELL ["/bin/bash", "-o", "pipefail", "-c"]

# Build toolchain for Ruby + V8
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    curl \
    git \
    pkg-config \
    ninja-build \
    python3 \
    libglib2.0-dev \
    mold \
    clang-19 \
    lld-19 \
    libclang-19-dev \
    libssl-dev \
    libyaml-dev \
    libreadline-dev \
    libffi-dev \
    zlib1g-dev \
    libgdbm-dev \
    libncurses-dev \
    && rm -rf /var/lib/apt/lists/*

# Compile Ruby 4.0.3 from source via ruby-build
RUN git clone --depth 1 https://github.com/rbenv/ruby-build.git /tmp/ruby-build \
    && /tmp/ruby-build/bin/ruby-build 4.0.3 /usr/local \
    && rm -rf /tmp/ruby-build

RUN gem install bundler

# Install Rust + sccache (V8 C++ compilation cache)
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --no-modify-path
ENV PATH=/root/.cargo/bin:$PATH
RUN cargo install sccache --locked

ENV SCCACHE=/root/.cargo/bin/sccache
ENV SCCACHE_DIR=/root/.cache/sccache
ENV RUSTFLAGS='-C link-arg=-fuse-ld=mold'

WORKDIR /app

COPY . .

# Generate a minimal JS bundle for the PoC (test/ excluded by .dockerignore)
RUN mkdir -p /app/test/fixtures && cat > /app/test/fixtures/minimal-bundle.js << 'EOF'
globalThis.render = function(data) {
  var parsed = typeof data === 'string' ? JSON.parse(data) : data;
  var name = (parsed.data && parsed.data.name) || 'world';
  return '<h1>' + name + '</h1>';
};
EOF

ENV V8_FROM_SOURCE=true
ENV GN_ARGS='v8_monolithic=true v8_monolithic_for_shared_library=true'
ENV LIBCLANG_PATH=/usr/lib/llvm-19/lib
ENV RB_SYS_CARGO_PROFILE=release

RUN --mount=type=cache,target=/root/.cargo/registry,sharing=locked \
    --mount=type=cache,target=/root/.cargo/git,sharing=locked \
    --mount=type=cache,target=/app/ext/ssr_deno/target,sharing=locked \
    --mount=type=cache,target=/root/.cache/sccache,sharing=locked \
    bundle install

RUN --mount=type=cache,target=/root/.cargo/registry,sharing=locked \
    --mount=type=cache,target=/root/.cargo/git,sharing=locked \
    --mount=type=cache,target=/app/ext/ssr_deno/target,sharing=locked \
    --mount=type=cache,target=/root/.cache/sccache,sharing=locked \
    bundle exec rake compile

RUN test -f lib/ssr/deno/ssr_deno.so && echo ".so OK" || (echo ".so MISSING" && exit 1)


# Stage 2: Minimal runtime (NO Rust, NO V8 source, NO LLVM)
FROM ubuntu:26.04

SHELL ["/bin/bash", "-o", "pipefail", "-c"]

# Runtime deps for Ruby binary (no build toolchain)
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    zlib1g \
    libyaml-0-2 \
    libffi8 \
    libgdbm6 \
    libncurses6 \
    && rm -rf /var/lib/apt/lists/*

# Copy compiled Ruby 4.0.3 from builder
COPY --from=builder /usr/local /usr/local
RUN ldconfig

WORKDIR /app

COPY --from=builder /app/lib /app/lib
COPY --from=builder /app/test/fixtures/minimal-bundle.js /app/minimal-bundle.js
COPY docker-entrypoint.rb /app/docker-entrypoint.rb

ENTRYPOINT ["ruby", "docker-entrypoint.rb"]
