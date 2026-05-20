# SSR::Deno

Server-side rendering for Ruby using Deno.

Embeds a Deno V8 runtime in Ruby via a Rust native extension. Loads Vite SSR
bundles (React, Vue, Svelte, Preact, vanilla TS) and calls their `render`
function — no subprocess, no HTTP bridge, no extra SSR server.

Rails users jump to [Using with Rails](#using-with-rails).

## Installation

```bash
bundle add ssr-deno
```

## Quick start

```ruby
File.write('/tmp/hello.js', <<~JS)
  globalThis.render = function (args) {
    var data = JSON.parse(args)
    return '<h1>Hello ' + (data.name || 'World') + '!</h1>'
  }
JS

bundle = SSR::Deno::Bundle.new('/tmp/hello.js')
html = bundle.render({ name: 'Deno SSR' })

puts html
# => <h1>Hello Deno SSR!</h1>
```

## Configuration

### Runtime settings

Set **before** creating any `SSR::Deno::Bundle` instance:

```ruby
SSR::Deno::Config.max_heap_size_mb = 128   # Per-isolate V8 heap (default: 64 MB)
SSR::Deno::Config.isolate_pool_size = 4    # V8 isolate count (default: 1)
SSR::Deno::Config.render_timeout_ms = 1000 # Render timeout (default: 500ms, min 100, max 300000)
SSR::Deno::Config.node_builtins_enabled = true  # Node.js built-in modules (default: false)
SSR::Deno::Config.source_maps_enabled = true  # Resolve V8 errors to original .tsx/.ts files (default: false)
```

```ruby
bundle.auto_reload = true  # Reload SSR bundle from disk when file mtime changes
```

The isolate pool distributes renders across V8 isolates in round-robin. Pool
size defaults to `1`. Multiple isolates benefit Ractor-based concurrency
(thread-based Rails apps also benefit — native_render releases the GVL during blocking I/O).

#### Environment variables

All runtime settings can also be configured via environment variables,
which act as **defaults** — explicit setter calls override them.

| Env var | Setting | Type | Default |
|---|---|---|---|
| `SSR_DENO_MAX_HEAP_SIZE_MB` | `max_heap_size_mb` | Integer (MB) | 64 |
| `SSR_DENO_ISOLATE_POOL_SIZE` | `isolate_pool_size` | Integer | 1 |
| `SSR_DENO_RENDER_TIMEOUT_MS` | `render_timeout_ms` | Integer (ms) | 500 |
| `SSR_DENO_NODE_BUILTINS_ENABLED` | `node_builtins_enabled` | Boolean | false |
| `SSR_DENO_SOURCE_MAPS_ENABLED` | `source_maps_enabled` | Boolean | false |

Boolean env vars accept `true`, `1`, `yes` (case-insensitive) for true;
anything else is treated as false. Invalid integer formats print a warning
and are skipped. Env vars are read once at `require 'ssr/deno'` time.

#### Node.js builtins

Enable when your SSR bundle or its dependencies call `require()` for `stream`,
`buffer`, `events`, etc. (e.g. `@emotion/server`). Adds ~50ms to worker init.
Must be set before pool init.

#### Source maps

When enabled, V8 stack traces from SSR render errors are resolved to original
`.tsx`/`.ts` source files instead of minified bundle positions. The gem reads
`.js.map` sidecars next to your bundles and corrects for the IIFE wrapper
offset used during bundle evaluation.

```ruby
SSR::Deno::Config.source_maps_enabled = true # or set SSR_DENO_SOURCE_MAPS_ENABLED=true
```

Best-effort — silently skips missing or corrupt `.map` files. On by default in
development and test Rails environments (`!Rails.env.production?`).

### Heap statistics

```ruby
SSR::Deno::HeapStats.fetch
# => { "total_heap_size" => 20971520, "used_heap_size" => 8388608, ... }
```

Returns 13 V8 memory counters from the isolate pool. Returns an empty Hash
with a warning if the runtime is not yet initialized. Use `fetch!` to
raise on error instead.

## Supported APIs

See [`docs/compatibility.md`](docs/compatibility.md) for detailed tables of:

- **Framework support** — which SSR frameworks and APIs work (React, Vue, Svelte, etc.)
- **JS API compatibility** — which standard, Web, and Node.js builtins are available
- **Known limitations** — macrotask starvation, SSR bundle footprint, heap limits, OOM behavior

## SSR bundle contract

Every SSR bundle must expose `globalThis.render(argsJson)`. It receives a JSON
string and must return an HTML string (or a Promise — the runtime detects async
and polls the V8 microtask queue until settlement).

For chunked streaming, the bundle can also push fragments via
`globalThis.__ssr_push_chunk(string)` during render — each call delivers one
HTML chunk to the Ruby `Enumerator`.

## Render usage

### Basic

```ruby
bundle = SSR::Deno::Bundle.new('dist/server/entry-server.js')
bundle.render({ page: 'home', user: @user })
```

### Raw Input

Pass a pre-serialized JSON string instead of a Ruby Hash — skips `JSON.generate`:

```ruby
bundle.render('{"page":"home"}', raw_input: true)
```

### Raw Output

Skip JSON parsing of the JS return value — get the raw string back:

```ruby
bundle.render({ page: 'home' }, raw_output: true)
```

Useful when the bundle returns a structured response like `JSON.stringify({html, css})` — you parse it yourself to inject CSS into `<head>`.

### Raw Input + Output

Both directions:

```ruby
bundle.render('{"page":"home"}', raw_input: true, raw_output: true)
```

### Event-loop render (async)

The V8 event loop always runs during render (macrotasks + microtasks fire). This means React 19 streaming SSR (`renderToPipeableStream`) works out of the box:

```ruby
bundle.render({ page: 'home' })
```

### Chunked render

Delivers HTML fragments incrementally as they arrive from JS. The JS bundle pushes chunks via `globalThis.__ssr_push_chunk(string)`:

```ruby
# Block form — yields each chunk
bundle.render_chunks({ page: 'home' }) do |chunk|
  response.stream.write(chunk)
end

# Enumerator form — Rack 3 compatible response body
body = bundle.render_chunks({ page: 'home' })
[200, { 'content-type' => 'text/html' }, body]
```

### CSP Nonce

Pass a nonce to the SSR bundle for Content Security Policy:

```ruby
bundle.render({ page: 'home', nonce: 'abc123' })
```

See [`docs/csp-nonce.md`](docs/csp-nonce.md) for JS-side usage and Emotion example.

## Error handling

All gem exceptions inherit from `SSR::Deno::Error` (`< StandardError`).
Rescue the base class to catch any gem error:

```ruby
rescue SSR::Deno::Error => e
  # covers all gem exceptions
end
```

Specific subclasses for targeted rescue:

| Class | When raised |
|-------|-------------|
| `SSR::Deno::RenderError` | JS render function throws or times out |
| `SSR::Deno::BundleNotFoundError` | named bundle not registered |
| `SSR::Deno::JsRuntimeWorkerError` | Deno worker thread died |
| `SSR::Deno::JsRuntimeOutOfMemoryError` | V8 heap limit exceeded |
| `SSR::Deno::JsRuntimeInitializationError` | config changed after init |
| `SSR::Deno::JsRuntimeNotInitializedError` | render called before init |
| `SSR::Deno::HeapStatsSerializationError` | heap stats JSON malformed |

## Using with Vite

The shared SSR build setup for all Vite-based samples:

```ts
import { defineConfig } from 'vite'

export default defineConfig({
  ssr: {
    target: 'webworker',
    noExternal: true,
    resolve: {
      conditions: ['edge-light', 'module', 'browser', 'development'],
    },
  },
  build: {
    ssr: true,
    outDir: 'dist/server',
    rollupOptions: { input: 'src/entry-server.ts' },
  },
})
```

Check the next section for examples and framework-specific setup.

## Experimental features

The gem ships two experimental features behind their own APIs. Both
work and are tested, but their interfaces and error semantics may
change without a deprecation cycle. **Not recommended for production.**
Please report bugs and rough edges at
<https://github.com/mdesantis/ssr-deno/issues> — feedback is what
unblocks the path to stable.

### Ractor pool (parallel SSR) — experimental

Parallel SSR via Ruby Ractors (Ruby 3.3+), bypassing the GVL bottleneck:

```ruby
SSR::Deno::Config.isolate_pool_size = 4
SSR::Deno::Config.node_builtins_enabled = true
pool = SSR::Deno::RactorPool.new(bundle_path: 'dist/server/ssr.js')
html = pool.render({ name: 'World' })

pool.render_chunks({ page: 'home' }) { |chunk| response.stream.write(chunk) }
pool.reload
pool.shutdown
```

Each Ractor pins a V8 isolate — renders execute in parallel. Bypasses
`ActiveSupport::Notifications` and the bundle registry; not compatible
with `SSR::Deno::Bundle` on the same isolate pool.

Full docs, requirements, limitations: [`docs/ractor-pool.md`](docs/ractor-pool.md).

### Dev mode (no-build SSR) — experimental

Load `.tsx` / `.ts` / `.js` source files directly — no Vite (or any)
bundler — with on-demand transpile and optional source-file auto-reload:

```ruby
SSR::Deno::Config.node_builtins_enabled = true
SSR::Deno::Config.source_maps_enabled  = true

bundle = SSR::Deno::DevModeBundle.new(
  'app/frontend/entry-server.tsx',
  name: :app,
  resolve_alias: { '@' => 'app/frontend' },
  project_root: Rails.root.to_s,
)
bundle.auto_reload = true

html = bundle.render({ page: 'home' })
```

Same `#render` / `#render_chunks` interface as `SSR::Deno::Bundle`; registers in
`SSR::Deno::Bundle.registry` so the Rails helper resolves it transparently.

Full docs, CJS interop notes, caveats: [`docs/dev-mode.md`](docs/dev-mode.md).

## Samples

The `samples/` directory contains several SSR samples. Run any with
`deno task build && deno task serve`:

| Port | Directory | Description |
|------|-----------|-------------|
| 3100 | [`barebone-ssr-app`](samples/barebone-ssr-app/) | Plain JS bundle, zero dependencies |
| 3101 | [`deno-native-ssr-app`](samples/deno-native-ssr-app/) | Deno.serve() + template strings, no build |
| 3102 | [`vite-ssr-app`](samples/vite-ssr-app/) | Vanilla TypeScript + Vite |
| 3103 | [`deno-native-react-ssr-app`](samples/deno-native-react-ssr-app/) | Deno.serve() + React 19, no build |
| 3104 | [`vite-svelte-ssr-app`](samples/vite-svelte-ssr-app/) | Svelte 5 + Vite |
| 3105 | [`vite-vue-ssr-app`](samples/vite-vue-ssr-app/) | Vue 3 + Vite |
| 3106 | [`vite-preact-ssr-app`](samples/vite-preact-ssr-app/) | Preact + Vite |
| 3107 | [`vite-react-ssr-app`](samples/vite-react-ssr-app/) | React 19 + Vite |
| 3108 | [`vite-react-mui-ssr-app`](samples/vite-react-mui-ssr-app/) | React 19 + MUI v9 + Vite |
| 3109 | [`vite-react-mui-emotion-ssr-app`](samples/vite-react-mui-emotion-ssr-app/) | React 19 + MUI v9 + Emotion CSS + Vite |
| 3110 | [`vite-react-emotion-mui-dashboard-ssr-app`](samples/vite-react-emotion-mui-dashboard-ssr-app/) | Full MUI dashboard + Vite |
| 3111 | [`webpack-ssr-app`](samples/webpack-ssr-app/) | Vanilla TypeScript + Webpack 5 |
| 3112 | [`webpack-react-ssr-app`](samples/webpack-react-ssr-app/) | React 19 + Webpack 5 |
| 3113 | [`node-ssr-app`](samples/node-ssr-app/) | Vanilla TypeScript + esbuild (Node.js) |
| 3114 | [`vite-react-streaming-ssr-app`](samples/vite-react-streaming-ssr-app/) | React 19 streaming SSR (renderToPipeableStream) + Vite |
| 3115 | [`vite-hmr-ssr-app`](samples/vite-hmr-ssr-app/) | Vite HMR development server |

Build all Vite samples at once:

```bash
bundle exec rake samples:build
```

## Using with Rails

### Setup

```bash
bundle add ssr-deno --require 'ssr/deno/rails'
bin/rails generate ssr:deno:install
```

### Configuration

In `config/initializers/ssr_deno.rb`:

```ruby
SSR::Deno.configure do |config|
  config.max_heap_size_mb = 128
  config.isolate_pool_size = 4
  config.render_timeout_ms = 1000
end
```

```ruby
# Raise on bundle errors in dev/test, fall back to CSR in production
Rails.application.config.ssr_deno.raise_on_bundle_error = false
# Emit heap stats notification every 50 renders
Rails.application.config.ssr_deno.heap_stats_sample_rate = 50
```

- `raise_on_bundle_error` (default: `true` in dev/test, `false` in production): when `false`, `BundleNotFoundError` logs, returns empty string (CSR fallback). Use `raise_on_render_error` for render errors.
- `raise_on_render_error` (default: `true` in dev/test, `false` in production): when `false`, `RenderError` logs, returns empty string.
- `source_maps_enabled` (default: `!Rails.env.production?`): resolve V8 errors to original `.tsx`/`.ts` files. Requires `.js.map` sidecars next to bundles.
- `heap_stats_sample_rate` (default: `100`): emit `heap_stats.ssr_deno` Active Support notification every N renders. Set to `0` to disable.

### Basic

`ssr_render` delegates to `SSR::Deno::Bundle#render` and accepts the same `raw_input:` and `raw_output:` options.

Rails auto-escapes HTML in views. Call `.html_safe` on the output if your bundle returns trusted HTML:

```erb
<%= ssr_render({ page: 'home', user: @user }).html_safe %>
```

Without `.html_safe`, special characters (`<`, `>`, `&`) are escaped by Rails.

### CSP Nonce

Pass nonce from `content_security_policy_nonce` helper:

```erb
<%= ssr_render({ page: "home", nonce: content_security_policy_nonce }) %>
```

See [CSP Nonce](#csp-nonce) for standalone usage and JS-side setup.

## Development

### Prerequisites

**All platforms**

- Ruby 3.3+
- Rust toolchain ([rustup](https://rustup.rs))
- Deno (for sample builds)

**Linux**

```bash
# LLVM (any recent version — used by bindgen only; V8 C++ uses Chromium's bundled clang)
# Replace 23 with whatever version your distro provides (19, 20, 21, 22, 23)
sudo apt-get install -y lld-23 clang-23 libclang-23-dev ninja-build

# Optional: faster linking
sudo apt-get install -y mold

# Optional: compiler cache (faster rebuilds)
sudo apt-get install -y sccache
```

**macOS**

```bash
# LLVM via Homebrew (used by bindgen only; V8 C++ uses Chromium's bundled clang)
brew install llvm ninja deno

# Optional: compiler cache (faster rebuilds)
brew install sccache
```

### Setup

```bash
git clone https://github.com/mdesantis/ssr-deno.git
cd ssr-deno
git submodule update --init --recursive
bin/setup # runs bundle install + copies .env.example → .env
```

After `bin/setup`, edit `.env` to match your platform:

- **Linux**: default `LIBCLANG_PATH=/usr/lib/llvm-23/lib` is correct if you installed clang-23.
- **macOS Apple Silicon**: uncomment `LIBCLANG_PATH=/opt/homebrew/opt/llvm/lib`, comment out the Linux line.
- **macOS Intel**: uncomment `LIBCLANG_PATH=/usr/local/opt/llvm/lib`, comment out the Linux line.
- **sccache** (optional): uncomment `SCCACHE=` and `RUSTC_WRAPPER=sccache` for faster subsequent builds.

See `.env.example` for all options and [`plans/v8-tls-issue.md`](plans/v8-tls-issue.md) for V8 build constraints.

### Compile

```bash
bundle exec rake compile
```

First compile downloads Chromium's clang toolchain and builds V8 from source
— expect 30–60 minutes. Subsequent builds are incremental (seconds with sccache).

### Run tests

```bash
bundle exec rake
```

Runs: Rust unit tests → Vite sample builds → Ruby tests → RuboCop → RBS
validation. Coverage must stay at 100% line + 100% branch.

## Architecture

See [`docs/architecture.md`](docs/architecture.md) for the component design,
data flow, and design decisions.

## Contributing

Bug reports and pull requests at https://github.com/mdesantis/ssr-deno.

## License

MIT — see [LICENSE.txt](LICENSE.txt).

## Code of Conduct

See [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
