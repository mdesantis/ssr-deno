# Rails test integration — Combustion

## Problem

`test/ssr/test_integration_deno_rails.rb` is dead code: no Rake task runs it,
no CI step invokes it, dummy app at `test/dummy/` is gitignored and only exists
locally as a hand-created `rails new` artifact.

The Railtie, Helper, and Generator have zero test coverage.

## Solution: Combustion

Replace hand-crafted dummy app with [Combustion](https://github.com/pat/combustion)
(1.6k stars, 30M+ downloads). Combustion creates an in-memory Rails app at
test-load time via `Combustion.initialize!`. `Combustion::Application` is a
pre-defined `Rails::Application` subclass inside the gem — no file generation.

**Why Combustion over `rails new` / Rake-task approach:**
- No separate Gemfile — Rails gems live in main Gemfile's test group
- No separate `bundle install` step — CI caches all gems together
- No Rake task to create/clean dummy — generated automatically
- Lightweight — only loads requested Rails components
- 30M+ downloads, maintained, widely used

## Files to create/modify

### `Gemfile`
Add to existing `:test` group (implicit, shared group):
```ruby
gem 'combustion', '~> 1.5'
gem 'rails', '~> 8.0'
```

`railties` is already a runtime dependency in `ssr-deno.gemspec` — no change needed.

### `test/test_helper_rails.rb` (new)
Rails-specific test helper. **Order is critical:**

SimpleCov start (must be first).
```ruby
require 'rails'                  # loads Rails module + Railtie + env/root/logger
require 'ssr/deno/rails'         # loads Railtie → registers initializers

require 'combustion'
Combustion.path = 'test/internal'  # explicit (defaults to spec/internal)
Combustion.initialize! :action_view, :action_controller  # boots Rails → runs initializers
```
Then Minitest/autorun.

**Why `require 'rails'` first:** The Railtie uses `Rails.env` at class-definition time
(`config.ssr_deno.auto_reload = Rails.env.development?`). `require 'rails/railtie'`
alone doesn't define `Rails.env` — need the full `rails.rb` from the `railties` gem.
Also makes `Rails::Railtie` available for `class Railtie < Rails::Railtie`.

**Why Railtie before Combustion:** Railtie initializers (setup `config.ssr_deno`,
include Helper in ActionView::Base) register at class-definition time. Rails
executes them during `initialize!`. Wrong order → initializers never run.

**`Combustion.path`:** Defaults to `/spec/internal`. Since Minitest isn't loaded
yet when `initialize!` runs (it's loaded after), Combustion can't detect the
framework. Set explicitly to `test/internal`.

**Bundler.require subtlety:** `Combustion.initialize!` calls `Bundler.require(:default, Rails.env)`.
For a gemspec gem named `ssr-deno`, Bundler infers `require 'ssr/deno'` — NOT
`require 'ssr/deno/rails'`. Safe: our step already loaded `ssr/deno` (via
`require_relative '../deno'`), so Bundler's require is a no-op.

### `rakelib/test.rake`
Add `test:rails` suite:
- Uses same pattern as `test:node_builtins` / `test:puma`
- Writes a runner script that requires `test_helper_rails.rb` + the test file
- Sets `SIMPLECOV_COMMAND_NAME=test:rails`
- Add `test:rails` to the default `test` task's dependency list

### `test/ssr/test_integration_deno_rails.rb`
- Remove `skip 'Rails dummy app not available'` guard (Combustion guarantees Rails)
- Update comment block with new run instructions (rake test:rails)

### `.rubocop.yml`
Add `test/internal/**/*` to Exclude list (Combustion-generated files).

### `test/test_helper.rb`
Add `add_filter 'test/internal/'` alongside existing `test/dummy/` filter.

### `test/support/integration_deno_rails_runner.rb` (deprecate)
Keep file but add deprecation notice pointing to Combustion. Runner is now
superceded by `test:rails` suite via `test_helper_rails.rb`.

### No CI changes needed
When `test:rails` is added to the `test` Rake task's dependency list, CI
automatically runs it via `bundle exec rake test`. No separate step.

## Edge cases

- **`require 'rails'` before Railtie**: The Railtie uses `Rails.env` at class-definition time
  (`config.ssr_deno.auto_reload = Rails.env.development?`). `require 'rails/railtie'` alone
  doesn't define `Rails.env`. Use `require 'rails'` (loads `railties/lib/rails.rb` which
  defines `Rails` module with `env`, `root`, `application`, etc.).

- **Railtie before Combustion.initialize!**: Railtie initializers register at class-definition time.
  `initialize!` runs them. Wrong order → initializers never run, `config.ssr_deno` missing,
  Helper not included in ActionView::Base, tests fail.

- **Bundler.require auto-require**: `Combustion.initialize!` calls `Bundler.require(:default, Rails.env)`.
  For gemspec gem `ssr-deno`, Bundler infers `require 'ssr/deno'` (not `ssr/deno/rails`).
  Already loaded → no-op. Railtie stays registered.

- **No file generation**: Combustion 1.5 uses a pre-defined in-memory `Rails::Application`
  subclass — no templates generated. Only Rails logger creates `test/internal/log/test.log`.

- **Coverage**: `SIMPLECOV_COMMAND_NAME=test:rails` ensures SimpleCov merges
  results with other suites. Existing `coverage:check` task validates merged result.

- **`railties` already a runtime dep**: gemspec lists it. Adding `rails` meta-gem
  to Gemfile test group pulls in `actionpack`, `actionview`, etc.

- **Gem caching**: All deps in main Gemfile → cached by `ruby/setup-ruby`
  `bundler-cache: true`. No extra CI config.

- **`EXCLUDED_MAIN` already covers `_deno_rails`**: test file excluded from `test:main` suite.

## Status

All steps complete ✅. 8 Rails integration tests now run via `rake test:rails`.

Key fixes discovered during implementation:
- `require 'rails'` not `require 'rails/railtie'` — Railtie uses `Rails.env` at class-definition time
- `Combustion.path = 'test/internal'` must be explicit — Minitest not loaded yet at `initialize!` call
- Added `test/internal/log/` to `.gitignore` — Rails logger creates log file there
