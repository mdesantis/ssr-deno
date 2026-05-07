# Merge deferred_bundles + Registry → unified Bundle.registry

## Goal

Drop `lib/ssr/deno/bundle/registry.rb`. `Bundle.registry` becomes a plain `{}`.
Eliminate `deferred_bundles` ivar and "deferred" naming everywhere.

Revised with review findings.

## Drop

- **`lib/ssr/deno/bundle/registry.rb`** — deleted
- **`deferred_bundles`** ivar + reader — gone entirely
- **`@_deferred_created`** → renamed to `@_bundles_created`
- **`create_deferred_bundles!`** → renamed to `create_bundles!`

## Change `@registry` from `Registry.new` to `{}`

All hash ops work natively: `[]`, `[]=`, `each`, `size`, `clear`.
No mutex on reads (reads via `attr_reader`). Only `create_bundles!` uses the
mutex via outer-guard + mutex pattern.

## `create_bundles!` — `transform_values!` + flag-after-transform

Fix from review: `dup+clear` had a race (empty window). `transform_values!`
replaces values in-place without changing keys — safe during MRI iteration.
`@_bundles_created` set AFTER transform completes so concurrent callers block
on mutex until registry is fully populated.

```ruby
def create_bundles!
  return if @_bundles_created

  @_create_mutex.synchronize do
    next if @_bundles_created

    @registry.transform_values! do |cfg|
      next cfg if cfg.is_a?(SSR::Deno::Bundle)

      bundle = new(cfg[:path])
      bundle.auto_reload = true if cfg[:auto_reload]
      bundle
    end

    @_bundles_created = true
  end
end
```

No `rescue ArgumentError` — plain hash assignment doesn't raise.
No retry logic needed in callers — concurrent threads block on mutex.

## Helper `find_bundle!` — `is_a?` check, not truthiness

Config hashes are truthy too. Must check `is_a?(SSR::Deno::Bundle)`:

```ruby
bundle = SSR::Deno::Bundle.registry[bundle_name]

unless bundle.is_a?(SSR::Deno::Bundle)
  SSR::Deno::Bundle.create_bundles!
  bundle = SSR::Deno::Bundle.registry[bundle_name]
end

unless bundle.is_a?(SSR::Deno::Bundle)
  raise SSR::Deno::BundleNotFoundError, ...
end
```

No retry needed — `create_bundles!` blocks on mutex until transform completes.

## Railtie

`Bundle.registry[name] = { path:, auto_reload: }` instead of
`Bundle.deferred_bundles[name]`.

## InstallGenerator

`Bundle.create_bundles!` instead of `Bundle.create_deferred_bundles!`.

## Tests

- All `Bundle.deferred_bundles[:x] = ...` → `Bundle.registry[:x] = ...`
- `teardown`: `Bundle.registry.clear` + `@_bundles_created = false`
- Remove `test_deferred_bundles_skips_already_registered` (no more
  `ArgumentError` on duplicates, `.register` doesn't exist on Hash)
- Rename remaining deferred-bundle tests to drop "deferred"
- Integration: `assert_instance_of Hash` for `.registry`,
  `assert_equal 0, Bundle.registry.size` stays same

## RBS

- Remove `Registry` class + all its methods
- `self.@registry`: union type —
  `Hash[Symbol, ({ path: String, auto_reload: boolish } | Bundle)]`
- `def self.create_bundles!: () -> void`
- Remove `self.@deferred_bundles` + `def self.deferred_bundles`

## Files touched

| File | What |
|------|------|
| `lib/ssr/deno/bundle/registry.rb` | Delete |
| `lib/ssr/deno/bundle.rb` | Remove require, remove deferred ivar, rename method+flag, replace register calls with `transform_values!` |
| `lib/ssr/deno/rails/helper.rb` | `is_a?` check, `create_bundles!` |
| `lib/ssr/deno/rails/railtie.rb` | `Bundle.registry[name] = { ... }` |
| `lib/ssr/deno/rails/generators/ssr/deno/install_generator.rb` | `create_bundles!` |
| `test/ssr/test_deno_bundle.rb` | Replace deferred calls, drop duplicate test, rename tests |
| `test/ssr/test_integration_deno_rails.rb` | Hash assertion |
| `sig/ssr/deno.rbs` | Remove Registry class, union type |
| `CHANGELOG.md` | Entry |
| `plans/archived/puma-lifecycle-init.md` | Update Delivered table — replace `deferred_bundles` refs |

## Verification

- `bundle exec rake` — exits 0
- Coverage 100% line + 100% branch
