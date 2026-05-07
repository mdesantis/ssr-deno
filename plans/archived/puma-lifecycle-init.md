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
`on_worker_boot { SSR::Deno::Bundle.create_deferred_bundles! }` to `config/puma.rb`.

## Solution: `Bundle.deferred_bundles` + `create_deferred_bundles!`

Railtie `init_bundles` stores bundle configs in `Bundle.deferred_bundles` but
does NOT call `Bundle.new`. `Bundle.create_deferred_bundles!` class method
reads the stored configs and creates/registers bundles (double-checked lock).

`InstallGenerator` appends `on_worker_boot` block to existing `config/puma.rb`.
Helper has lazy fallback for single-mode (creates bundles on first render).

## Delivered

| File | Change |
|------|--------|
| `lib/ssr/deno/bundle.rb` | `deferred_bundles`, `create_deferred_bundles!` class methods |
| `lib/ssr/deno/rails/railtie.rb` | `init_bundles` stores config in `Bundle.deferred_bundles` |
| `lib/ssr/deno/rails/helper.rb` | `find_bundle!` lazy fallback calls `create_deferred_bundles!` |
| `lib/ssr/deno/rails/generators/ssr/deno/install_generator.rb` | `add_puma_on_worker_boot` |
| `sig/ssr/deno.rbs` | signatures |
| `CHANGELOG.md` | entries |
| `test/ssr/test_deno_bundle.rb` | deferred bundles tests, double-check lock test, auto_reload test, duplicate registration test |

## Verification

- `bundle exec rake` — exits 0 ✅
- Coverage 100% line + 100% branch ✅
- Puma integration tests (single + clustered) pass ✅
