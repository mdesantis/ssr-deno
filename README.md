# ssr-deno

Server-side rendering for Ruby using Deno.

`ssr-deno` embeds a Deno V8 runtime in Ruby via a Rust native extension, enabling server-side rendering of JavaScript/TypeScript frameworks (React, Vue, etc.) directly from Ruby.

## Installation

Add this line to your application's Gemfile:

```ruby
gem 'ssr-deno'
```

And then execute:

```bash
$ bundle install
```

Or install it yourself as:

```bash
$ gem install ssr-deno
```

## Usage

```ruby
require 'ssr/deno'

# Create a bundle from a Vite SSR production build
bundle = SSR::Deno::Bundle.new('path/to/dist/server/entry-server.js')

# Render a component — data is automatically JSON-serialized
html = bundle.render({
  data: { message: 'Hello World!' }
})

puts html
# => <html><head><title></title></head><body>...
```

The `render` method accepts a Hash with arbitrary data, which is serialized to JSON and passed to the SSR bundle's `render` function. Multiple bundles can coexist in the same process:

```ruby
application = SSR::Deno::Bundle.new('dist/server/entry-server.js')
admin       = SSR::Deno::Bundle.new('dist/server/admin/entry-server.js')

application.render({ page: 'home' })
admin.render({ page: 'dashboard' })
```

### Configuration

Configure the V8 heap limit and isolate pool **before** creating any `Bundle` instances:

```ruby
SSR::Deno.max_heap_size_mb = 128   # Per-isolate heap limit (default: 64 MB)
SSR::Deno.isolate_pool_size = 4    # Number of V8 isolates (0 = auto-detect)
```

The isolate pool distributes render requests across multiple V8 isolates in round-robin fashion, enabling parallel SSR within a single Ruby process. Pool size defaults to `CPU_cores - 1` (capped at 8), reserving one core for the Ruby thread.

Each isolate gets its own V8 heap (configured by `max_heap_size_mb`), its own Deno runtime, and its own worker thread. Render requests are dispatched without locks — just atomic counter increment + channel send.

### Creating SSR bundles

`ssr-deno` loads Vite SSR production bundles and calls their `render` function. Each bundle must expose a `globalThis.render(argsJson: string): string` function. The `samples/` directory contains complete working examples for each framework.

### Bundle contract

```
globalThis.render(argsJson: string): string
```

Arguments are passed as a JSON string. The return value must be a complete HTML string (or a Promise that resolves to one — the Rust runtime auto-detects async render functions and polls the V8 microtask queue until settlement).

### Vanilla (no framework)

```ts
// src/entry-server.ts — plain TypeScript, no framework
function render(argsJson: string): string {
  const { name } = JSON.parse(argsJson)
  return `<!DOCTYPE html>
<html>
  <head><title>Hello ${name}</title></head>
  <body>
    <div id="root"><h1>Hello ${name}!</h1></div>
  </body>
</html>`
}

globalThis.render = render
```

Full sample: [`samples/vanilla-ssr-app/`](samples/vanilla-ssr-app/)

### Vue 3

```ts
// src/entry-server.ts
import { createSSRApp } from 'vue'
import { renderToString } from 'vue/server-renderer'
import App from './App.vue'

async function render(argsJson: string): Promise<string> {
  const { data } = JSON.parse(argsJson)
  const app = createSSRApp(App, { data })
  const body = await renderToString(app)
  return `<!DOCTYPE html>
<html>
  <head><title>Hello</title></head>
  <body><div id="root">${body}</div></body>
</html>`
}

globalThis.render = render
```

Vue's `renderToString` returns a Promise — async render functions are handled transparently.

Full sample: [`samples/vue-ssr-app/`](samples/vue-ssr-app/)

### Svelte 5

```ts
// src/entry-server.ts
import { render as renderSvelte } from 'svelte/server'
import App from './App.svelte'

function render(argsJson: string): string {
  const { data } = JSON.parse(argsJson)
  const result = renderSvelte(App, { props: { data } })
  return `<!DOCTYPE html>
<html>
  <head>${result.head}<title>Hello</title></head>
  <body><div id="root">${result.body}</div></body>
</html>`
}

globalThis.render = render
```

Svelte 5's `render` from `svelte/server` is synchronous and returns `{ head, body }`.

Full sample: [`samples/svelte-ssr-app/`](samples/svelte-ssr-app/)

### React 19

```tsx
// src/entry-server.tsx
import { renderToString } from 'react-dom/server'
import { createElement } from 'react'
import App from './App'

function render(argsJson: string): string {
  const { data } = JSON.parse(argsJson)
  const html = renderToString(createElement(App, { data }))
  return `<!DOCTYPE html>
<html>
  <head><title>Hello</title></head>
  <body><div id="root">${html}</div></body>
</html>`
}

globalThis.render = render
```

React's `renderToString` is synchronous.

Full sample: [`samples/react-ssr-app/`](samples/react-ssr-app/)

### Vite configuration

All samples use the same Vite SSR build setup. Framework-specific builds add their respective Vite plugin:

```ts
import { defineConfig } from 'vite'

export default defineConfig({
  ssr: {
    target: 'webworker',
    noExternal: true,          // bundle all dependencies
  },
  build: {
    ssr: true,
    outDir: 'dist/server',
    rollupOptions: {
      input: 'src/entry-server.ts',
    },
  },
})
```

| Framework | Vite plugin |
|-----------|-------------|
| Vue 3 | `@vitejs/plugin-vue` |
| Svelte 5 | `@sveltejs/vite-plugin-svelte` |
| React 19 | `@vitejs/plugin-react` |

All samples also add `ssr.resolve.conditions: ['edge-light', 'module', 'browser', 'development']`
to prevent bundler from resolving packages (like `@emotion/cache`) to their browser-specific builds
when `ssr.target: 'webworker'` is set. See
[`plans/edge-light-resolution.md`](plans/edge-light-resolution.md) for details.

### Building and running

Each sample defines `deno task build` and `deno task serve` in its `deno.json`:

```bash
cd samples/vanilla-ssr-app
deno task build                # produces dist/server/entry-server.js
deno task serve                # starts a test server on localhost:3100
```

Build all samples at once:

```bash
bundle exec rake samples:build
```

### Loading a bundle in Ruby

```ruby
require 'ssr/deno'

# Point to the built entry file
bundle = SSR::Deno::Bundle.new('samples/vanilla-ssr-app/dist/server/entry-server.js')

# Data is auto-serialized to JSON and passed to the render function
html = bundle.render({ name: 'World' })
puts html
# => <!DOCTYPE html>\n<html>...
```

## Rails integration

Add to your Gemfile:

```ruby
gem 'ssr-deno', require: 'ssr/deno/rails'
```

Then run the generator:

```bash
rails generate ssr:deno:install
```

Use the `ssr_render` helper in your views:

```erb
<%= ssr_render({ page: 'home', user: @user }) %>
```

Configure via the Rails generator initializer:

```ruby
# config/initializers/ssr_deno.rb
SSR::Deno.configure do |config|
  config.max_heap_size_mb = 128
  config.isolate_pool_size = 4  # nil = auto-detect (CPU cores - 1, max 8)
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
bin/setup
```

### Compile the native extension

Copy `.env.example` to `.env` before the first compile — without it, the build
falls back to release mode (slow) and is missing required V8 build variables:

```bash
cp .env.example .env
bundle exec rake compile
```

`.env` configures, via `dotenv`:

- `V8_FROM_SOURCE`, `GN_ARGS`, `LIBCLANG_PATH` — required to build V8 as a
  shared library (see [`plans/v8-tls-issue.md`](plans/v8-tls-issue.md)).
- `RB_SYS_CARGO_PROFILE=dev` — fast iterative builds, suitable for
  `rake test`. Switch to `release` for a shipping artifact.
- Optional `RUSTFLAGS` (`mold` linker) and `SCCACHE` for further speedups.

Adjust the paths for your system after copying.

### Run tests

```bash
bundle exec rake test
```

### Interactive console

```bash
bin/console
```

## Architecture

See [`plans/architecture.md`](plans/architecture.md) for a detailed overview of the project architecture, component design, and data flow.

## Contributing

Bug reports and pull requests are welcome on GitHub at https://github.com/mdesantis/ssr-deno.

## License

The gem is available as open source under the terms of the [MIT License](https://opensource.org/licenses/MIT).

## Code of Conduct

Everyone interacting in the ssr-deno project's codebases, issue trackers, chat rooms and mailing lists is expected to follow the [code of conduct](https://github.com/mdesantis/ssr-deno/blob/main/CODE_OF_CONDUCT.md).
