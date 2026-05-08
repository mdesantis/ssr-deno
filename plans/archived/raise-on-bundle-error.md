# raise-on-bundle-error

## Problem

`lazy_register` + `attempt_lazy_register` do wrong thing: find bundles on disk when missing at boot. Real concern is **error handling** during bundle loading/registration (Puma-deferred `Bundle.new`) — missing files at boot and unknown bundle names at render should raise in dev/test, log in production.

## Changes

### 1. `lib/ssr/deno/rails/helper.rb` — remove `attempt_lazy_register`/`bundle_path_for`

Revert `find_bundle!` to pre-lazy_register:

```
registry lookup → create_bundles! → BundleNotFoundError
```

Remove `attempt_lazy_register`, `bundle_path_for` methods entirely. Keep `instrument` (used by `BundleNotFoundError` raise path).

### 2. `lib/ssr/deno/rails/railtie.rb` — remove `lazy_register`, add `raise_on_bundle_error`

Delete line `config.ssr_deno.lazy_register = Rails.env.production?`. Add near `raise_on_render_error`:

```ruby
config.ssr_deno.raise_on_bundle_error = !Rails.env.production?
```

### 3. `init_bundles` — `logger.warn` → `logger.error`

Change level unconditionally (no conditional raise — see coverage note below):

```ruby
unless File.exist?(path)
  Rails.logger.error "[ssr-deno] Bundle #{name.inspect} not found at #{path}. Skipping."
  next
end
```

**Coverage note:** Adding a conditional raise here (`raise if config.ssr_deno.raise_on_bundle_error`) creates an uncovered branch. The Combustion test app must have `bundles = {}` (no configured bundles) to avoid crashing at boot — but then the raise path is never exercised. The `error` level is sufficient: missing bundle IS an error, surfacing at render time via `BundleNotFoundError` is the actual gate. Consistent with `raise_on_render_error` which raises at the error site, not earlier.

### 4. `ssr_render` — rescue `BundleNotFoundError`

Parallel to `raise_on_render_error`:

```ruby
rescue SSR::Deno::BundleNotFoundError => error
  raise if Rails.application.config.ssr_deno.raise_on_bundle_error

  Rails.logger.error "[ssr-deno] Bundle #{bundle_name.inspect} not found, " \
                     "falling back to CSR: #{error.message}"
  ''
```

### 5. `lib/ssr/deno/rails/generators/.../ssr_deno.rb` — replace `lazy_register` with `raise_on_bundle_error`

```ruby
# Raise on bundle not found (recommended: true in dev/test, false in production).
# Rails.application.config.ssr_deno.raise_on_bundle_error = !Rails.env.production?
```

### 6. `test/internal/config/initializers/ssr_deno.rb` — clear bundles for Combustion app

Create initializer so `init_bundles` doesn't raise at boot:

```ruby
Rails.application.config.ssr_deno.bundles = {}
```

### 7. `test/ssr/test_integration_deno_rails.rb` — add `raise_on_bundle_error` assertion

```ruby
assert Rails.application.config.ssr_deno.raise_on_bundle_error
```

### 8. CHANGELOG.md

Remove Added entry for `lazy_register`. Add entry:

```
- `config.ssr_deno.raise_on_bundle_error` — when true (default in dev/test), `BundleNotFoundError` at render raises. When false (production), caught and logged with CSR fallback (empty string). Defaults to `!Rails.env.production?`.
```

### 9. README.md

Remove "Lazy bundle registration" section. Add "Bundle error handling" section.

### 10. `lib/ssr/deno/rails/railtie.rb` — revert `default_bundle_path` to private instance method

No longer needs class-method form (only `init_bundles` calls it now). Revert to original:

```ruby
private

def default_bundle_path(name)
  Rails.root.join("dist/server/#{name}/entry-server.js")
end
```

### 11. `sig/ssr/deno.rbs`

Remove `attempt_lazy_register`, `bundle_path_for` from Helper. Revert `default_bundle_path` to private instance method on Railtie.
