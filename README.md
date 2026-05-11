# ssr-deno

Server-side rendering for Ruby using Deno.

Embeds a Deno V8 runtime in Ruby via a Rust native extension. Loads Vite SSR
bundles (React, Vue, Svelte, Preact, vanilla TS) and calls their `render`
function — no subprocess, no HTTP bridge, no Node.js.

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

Set **before** creating any `Bundle` instance:

```ruby
SSR::Deno.max_heap_size_mb = 128   # Per-isolate V8 heap (default: 64 MB)
SSR::Deno.isolate_pool_size = 4    # V8 isolate count (default: 1)
SSR::Deno.render_timeout_ms = 1000 # Render timeout (default: 500ms, min 100, max 300000)
SSR::Deno.node_builtins_enabled = true  # Node.js built-in modules (default: false)
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

Boolean env vars accept `true`, `1`, `yes` (case-insensitive) for true;
anything else is treated as false. Invalid integer formats print a warning
and are skipped. Env vars are read once at `require 'ssr/deno'` time.

#### Node.js builtins

Enable when your SSR bundle or its dependencies call `require()` for `stream`,
`buffer`, `events`, etc. (e.g. `@emotion/server`). Adds ~50ms to worker init.
Must be set before pool init.

### Heap statistics

```ruby
SSR::Deno.heap_stats
# => { "total_heap_size" => 20971520, "used_heap_size" => 8388608, ... }
```

Returns 13 V8 memory counters from the isolate pool. Returns an empty Hash
with a warning if the runtime is not yet initialized. Use `heap_stats!` to
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
- `heap_stats_sample_rate` (default: `100`): emit `heap_stats.ssr_deno` Active Support notification every N renders. Set to `0` to disable.

### Basic

`ssr_render` delegates to `Bundle#render` and accepts the same `raw_input:` and `raw_output:` options.

Rails auto-escapes HTML in views. Call `.html_safe` on the output if your bundle returns trusted HTML:

```erb
<%= ssr_render({ page: 'home', user: @user }).html_safe %>
```

Without `.html_safe`, special characters (`<`, `>`, `&`) are escaped by Rails.

### CSP Nonce

Pass nonce via `ssr_render` data hash:

```erb
<%= ssr_render({ page: "home", nonce: content_security_policy_nonce }) %>
```

See [`docs/csp-nonce.md`](docs/csp-nonce.md) for JS-side usage and Emotion example.

## Ractor pool (parallel SSR)

For concurrent SSR under Ractors (Ruby 3.3+) without the GVL bottleneck:

```ruby
SSR::Deno.isolate_pool_size = 4
SSR::Deno.node_builtins_enabled = true
pool = SSR::Deno::RactorPool.new(bundle_path: 'dist/server/ssr.js')
html = pool.render({ name: 'World' })
```

Each Ractor runs a V8 isolate — renders execute in parallel. Not compatible with `SSR::Deno::Bundle`; use one or the other.

```ruby
pool.render_chunks({ page: 'home' }) { |chunk| response.stream.write(chunk) }
pool.reload
pool.shutdown
```

## Development

### Prerequisites

- Ruby 3.3+
- Rust toolchain
- LLVM/Clang 21 (for V8 build)
- Bundler

### Setup

```bash
git clone https://github.com/mdesantis/ssr-deno.git
cd ssr-deno
bin/setup # Will also run `cp .env.example .env`
```

### Compile

```bash
bundle exec rake compile
```

`.env` configures V8 build variables (`V8_FROM_SOURCE`, `GN_ARGS`,
`LIBCLANG_PATH`) and `RB_SYS_CARGO_PROFILE`. See
[`plans/v8-tls-issue.md`](plans/v8-tls-issue.md) for the V8 build constraints.

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
