# ssr-deno

Server-side rendering for Ruby using Deno.

Embeds a Deno V8 runtime in Ruby via a Rust native extension. Loads Vite SSR
bundles (React, Vue, Svelte, Preact, vanilla TS) and calls their `render`
function — no subprocess, no HTTP bridge, no Node.js.

## Installation

```bash
bundle add 'ssr-deno'
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

Set **before** creating any `Bundle` instance:

```ruby
SSR::Deno.max_heap_size_mb = 128   # Per-isolate V8 heap (default: 64 MB)
SSR::Deno.isolate_pool_size = 4    # V8 isolate count (0 = auto-detect)
SSR::Deno.render_timeout_ms = 1000 # Render timeout (default: 500ms, min 100, max 300000)
SSR::Deno.node_builtins_enabled = true  # Node.js built-in modules (default: false)
```

The isolate pool distributes renders across V8 isolates in round-robin. Pool
size defaults to `CPU_cores - 1` (capped at 8), leaving one core for Ruby.

```ruby
bundle.auto_reload = true  # Reload bundle from disk when file mtime changes
```

### Node.js builtins

Enable when your bundle or its dependencies call `require()` for `stream`,
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

## Bundle contract

Every SSR bundle must expose `globalThis.render(argsJson)`. It receives a JSON
string and must return an HTML string (or a Promise — the runtime detects async
and polls the V8 microtask queue until settlement).

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

Framework-specific builds add their Vite plugin:

| Framework | Plugin |
|-----------|--------|
| Vue 3 | `@vitejs/plugin-vue` |
| Svelte 5 | `@sveltejs/vite-plugin-svelte` |
| React 19 / Preact | `@vitejs/plugin-react` (Preact uses `resolve.alias` instead — see `samples/vite-preact-ssr-app/vite.config.ts`) |

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

Build all Vite samples at once:

```bash
bundle exec rake samples:build
```

## Rails integration

```ruby
gem 'ssr-deno', require: 'ssr/deno/rails'
```

```bash
rails generate ssr:deno:install
```

```erb
<%= ssr_render({ page: 'home', user: @user }) %>
```

Configure in `config/initializers/ssr_deno.rb`:

```ruby
SSR::Deno.configure do |config|
  config.max_heap_size_mb = 128
  config.isolate_pool_size = 4
  config.render_timeout_ms = 1000
end
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
cp .env.example .env
bin/setup
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

### Console

```bash
bin/console
```

## Architecture

See [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) for the component design,
data flow, and design decisions.

## Contributing

Bug reports and pull requests at https://github.com/mdesantis/ssr-deno.

## License

MIT — see [LICENSE.txt](LICENSE.txt).

## Code of Conduct

See [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
