# Rails test integration — Combustion

## Problem

`test/ssr/test_integration_deno_rails.rb` is dead code: no Rake task runs it,
no CI step invokes it, dummy app at `test/dummy/` is gitignored and only exists
locally as a hand-created `rails new` artifact.

The Railtie, Helper, and Generator have zero test coverage.

## Solution: Combustion

Replace hand-crafted dummy app with [Combustion](https://github.com/pat/combustion)
(1.6k stars, 30M+ downloads). Combustion creates an in-memory Rails app at
test-load time via `Combustion.initialize!`, generating minimal config files
at `test/internal/` on first run.

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
require 'rails/railtie'          # makes Rails::Railtie available
require 'ssr/deno/rails'         # loads Railtie → registers initializers
Combustion.initialize! :action_view, :action_controller  # boots Rails → runs initializers
```
Then Minitest/autorun.

**Why `require 'rails/railtie'` first:** `ssr/deno/rails` → `rails/railtie.rb` →
`class Railtie < Rails::Railtie`. Without it, `Rails::Railtie` is undefined
(`railties` is installed but not loaded yet). `NameError`.

**Why Railtie before Combustion:** Railtie initializers (setup `config.ssr_deno`,
include Helper in ActionView::Base) register at class-definition time. Rails
executes them during `initialize!`. Wrong order → initializers never run.

**Bundler.require subtlety:** Combustion's generated `application.rb` calls
`Bundler.require(*Rails.groups)`. For a gemspec gem named `ssr-deno`, Bundler
infers `require 'ssr/deno'` — NOT `require 'ssr/deno/rails'`. This is safe:
our step already loaded `ssr/deno` (via `require_relative '../deno'`), so
Bundler's require is a no-op. The Railtie stays registered.

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

- **`require 'rails/railtie'` before Railtie**: `ssr/deno/rails` → `class Railtie < Rails::Railtie`.
  `railties` gem is installed (runtime dep) but NOT loaded — must `require 'rails/railtie'` first.
  Without it → `NameError` (uninitialized constant Rails).

- **Railtie before Combustion.initialize!**: Railtie initializers register at class-definition time.
  `initialize!` runs them. Wrong order → initializers never run, `config.ssr_deno` missing,
  Helper not included in ActionView::Base, tests fail.

- **Bundler.require auto-require**: Combustion's `application.rb` calls `Bundler.require`.
  For gemspec gem `ssr-deno`, Bundler infers `require 'ssr/deno'` (not `ssr/deno/rails`).
  Already loaded → no-op. Railtie stays registered.

- **`test/internal/` generation**: Combustion generates files on first `initialize!`
  call (subprocess). Expected: `config/application.rb`, `config/database.yml`,
  `config/routes.rb`, `config/boot.rb`, `app/views/layouts/application.html.erb`.
  Commit these to repo.

- **Coverage**: `SIMPLECOV_COMMAND_NAME=test:rails` ensures SimpleCov merges
  results with other suites. Existing `coverage:check` task validates merged result.

- **`railties` already a runtime dep**: gemspec lists it. Adding `rails` meta-gem
  to Gemfile test group pulls in `actionpack`, `actionview`, etc.

- **Gem caching**: All deps in main Gemfile → cached by `ruby/setup-ruby`
  `bundler-cache: true`. No extra CI config.

- **`EXCLUDED_MAIN` already covers `_deno_rails`**: test file excluded from `test:main` suite.

## Implementation order

1. Edit `Gemfile` — add `combustion`, `rails`
2. Create `test/test_helper_rails.rb`
3. Edit `rakelib/test.rake` — add `test:rails` suite
4. Edit `test/ssr/test_integration_deno_rails.rb` — remove skip guard
5. Edit `.rubocop.yml` — add `test/internal/` exclusion
6. Edit `test/test_helper.rb` — add `test/internal/` filter
7. Mark `test/support/integration_deno_rails_runner.rb` deprecated
8. `bundle install`
9. `bundle exec rake` — compile + test + lint
10. Commit
