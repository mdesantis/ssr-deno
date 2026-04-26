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

# Initialize the runtime with a Vite SSR bundle
result = SSR::Deno.init_runtime('path/to/dist/server/entry-server.js')
# => true  (first call)
# => nil   (subsequent calls)

# Render a component
html = SSR::Deno.render({
  component_data: { message: 'Hello World!' },
  props: {},
  url: '/'
})

puts html
# => <html><head><title></title></head><body>...
```

The `render` function accepts a Hash with arbitrary data, which is serialized to JSON and passed to the SSR bundle's `render` function.

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

```bash
./bin/compile
```

> **Note:** The `compile` Rake task **must** be run through `./bin/compile`, which sets the environment variables required to build V8 as a shared library (see [`plans/v8-tls-issue.md`](plans/v8-tls-issue.md)).

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
