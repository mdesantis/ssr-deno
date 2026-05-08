# Remove `railties` as hard runtime dependency

_target: `ssr-deno.gemspec`, `lib/ssr/deno/rails.rb`_

## Problem

`ssr-deno.gemspec:35` declares `spec.add_dependency 'railties'` — a **hard** runtime
dependency. Every user who installs `ssr-deno` gets `railties` pulled in, even if
they don't use Rails integration. The comment says "optional" but RubyGems has no
optional dependency concept — `add_dependency` always installs.

## Current usage of railties

| File | Uses | Loaded when |
|------|------|-------------|
| `lib/ssr/deno/rails/railtie.rb` | `Rails::Railtie`, `ActiveSupport::OrderedOptions`, `ActiveSupport::Notifications`, etc. | `require: 'ssr/deno/rails'` |
| `lib/ssr/deno/rails/helper.rb` | `Rails.application.config`, `ActiveSupport::Notifications` | `require: 'ssr/deno/rails'` |
| `lib/ssr/deno/rails/generators/.../install_generator.rb` | `Rails::Generators::Base` | `require: 'ssr/deno/rails'` |
| `lib/ssr/deno/rails.rb` (entry point) | requires above files | `require: 'ssr/deno/rails'` |
| `lib/ssr/deno/instrumenter.rb` | `defined?(ActiveSupport::Notifications)` — graceful, no hard dep | always loaded, but guarded |

**Core gem files** (`lib/ssr/deno.rb`, `bundle.rb`, `instrumenter.rb`) — zero Rails
dependency. Only the on-demand `lib/ssr/deno/rails/` tree needs it.

## Changes

### 1. `ssr-deno.gemspec`

```
-  spec.add_dependency 'railties' # optional — only loaded when require: 'ssr/deno/rails'
```

### 2. `lib/ssr/deno/rails.rb`

Add a guard at the top before loading Rails files. Without `railties` in the
Gemfile, `class Railtie < Rails::Railtie` raises `NameError: uninitialized
constant Rails` — cryptic. Replace with a clear message:

```ruby
unless defined?(Rails::Railtie)
  raise LoadError, <<~MSG.strip
    [ssr-deno] Rails integration requires the railties gem.
    Add `gem 'railties'` to your Gemfile, or use `gem 'ssr-deno', require: 'ssr/deno/rails'`
    (which loads railties automatically via the gemspec — removed in a future version).
  MSG
end
```

Wait — if we remove `add_dependency 'railties'`, then `gem 'ssr-deno', require: 'ssr/deno/rails'` won't pull in `railties` either. The message should just say:

```ruby
unless defined?(Rails::Railtie)
  raise LoadError, '[ssr-deno] Rails integration requires railties. '\
                    'Add gem "railties" to your Gemfile.'
end
```

### 3. `CHANGELOG.md`

Add entry under `### Removed` in `## Unreleased`.

### 4. Stale audit

Search for stale references to the `railties` dependency:

- Comments in `ssr-deno.gemspec` — remove or update
- `plans/archived/rails-dummy-app-setup.md` — mentions `railties` is a runtime dep
- `plans/archived/rails-integration.md` — mentions optional dep decision
- `lib/ssr/deno/rails/railtie.rb` — verify no stale comments
- `CHANGELOG.md` — verify old entries don't need updating

## Not changing

- **`Gemfile`** — already has `gem 'rails', '~> 8.0', require: false` for dev/test
- **`lib/ssr/deno/instrumenter.rb`** — already guarded with `defined?`
- **`sig/ssr/deno.rbs`** — only mentions Rails in a comment, no dep issue
- **Test suite** — unaffected, `Gemfile` pulls in `rails`

## Guard implementation note

`defined?(Rails::Railtie)` works because:
- `Rails` constant is defined when `railties` is loaded
- `Railtie` is its subclass — always available when `Rails` is
- If someone loads `activesupport` but not `railties`, `Rails` won't be defined
  (it lives in `railties/lib/rails.rb`), so the guard correctly catches both
  "no Rails at all" and "partial Rails" cases

## Migration impact

Existing users who already have `railties` in their Gemfile (e.g., via `gem 'rails'`
or transitive deps) — no change, everything works.

Users who relied on `ssr-deno` pulling in `railties` implicitly:
- If they don't use Rails integration: no change, they get a smaller dependency tree
- If they do use Rails integration: they'll get the LoadError with the fix message.
  They add `gem 'railties'` and continue.
