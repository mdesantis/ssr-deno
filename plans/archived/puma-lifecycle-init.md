# Plan: Puma lifecycle init — `on_worker_boot` approach

**SUPERSEDED — implemented.** All deferred-bundle work landed on `Bundle` class
per user preference (not `Railtie`). See archived plan below for details.

Archived at: `plans/archived/puma-lifecycle-init.md`

## Context

Railtie calls `Bundle.new` during boot (`init_bundles` initializer). Puma
`preload_app!` (Rails production default) loads app in master → forks. V8
isolates created before fork are corrupted (V8 TLS limitation).

Archived `puma-v8-limitation.md` says: "Defer Bundle.new to on_worker_boot."
The correct fix is the Railtie defers bundle creation and users add
`on_worker_boot { SSR::Deno::Bundle.create_bundles! }` to `config/puma.rb`.

## Solution: `Bundle.registry` unified store (was `deferred_bundles` + `Registry`)

Railtie `init_bundles` stores bundle configs in `Bundle.registry` (plain `{}`)
but does NOT call `Bundle.new`. `Bundle.create_bundles!` class method
transforms config hashes into Bundle instances in-place via `transform_values!`.

`InstallGenerator` appends `on_worker_boot` block to existing `config/puma.rb`.
Helper has lazy fallback for single-mode (creates bundles on first render).

## Delivered

| File | Change |
|------|--------|
| `lib/ssr/deno/bundle.rb` | `registry` (plain Hash), `create_bundles!` class method |
| `lib/ssr/deno/rails/railtie.rb` | `init_bundles` stores config in `Bundle.registry` |
| `lib/ssr/deno/rails/helper.rb` | `find_bundle!` lazy fallback calls `create_bundles!` |
| `lib/ssr/deno/rails/generators/ssr/deno/install_generator.rb` | `add_puma_on_worker_boot` |
| `sig/ssr/deno.rbs` | signatures |
| `CHANGELOG.md` | entries |
| `test/ssr/test_deno_bundle.rb` | registry tests, create_bundles guard test, auto_reload test |

## Verification

- `bundle exec rake` — exits 0 ✅
- Coverage 100% line + 100% branch ✅
- Puma integration tests (single + clustered) pass ✅
