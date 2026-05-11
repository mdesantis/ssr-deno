# Extract SSR::Deno::Config module

Move config setters/getters/env-defaults out of `SSR::Deno` into dedicated `SSR::Deno::Config` module. Native FFI methods stay on `SSR::Deno`.

## Files to create

- `lib/ssr/deno/config.rb` — `SSR::Deno::Config` module with:
  - `max_heap_size_mb=`, `isolate_pool_size=`, `render_timeout_ms=`, `node_builtins_enabled=`
  - `max_heap_size_mb`, `isolate_pool_size`, `render_timeout_ms`, `node_builtins_enabled?`
  - `apply_env_var_defaults`, `apply_integer_env`, `apply_bool_env`

## Files to modify

### Core
1. **`lib/ssr/deno.rb`** — strip config methods (~55 lines), add `require_relative 'deno/config'`, call `SSR::Deno::Config.apply_env_var_defaults`

### Internal libs
2. **`lib/ssr/deno/rails/railtie.rb`** — 4 lines: `SSR::Deno.max_heap_size_mb=` → `SSR::Deno::Config.max_heap_size_mb=`
3. **`lib/ssr/deno/ractor_pool.rb`** — doc comments only (2 lines)

### Rake/scripts
4. **`rakelib/test.rake`** — ~11 calls
5. **`rakelib/perf.rake`** — 3 calls
6. **`scripts/throughput.rb`** — 3 calls
7. **`scripts/performance.rb`** — 3 calls
8. **`Dockerfile`** — 2 lines

### Tests
9. **`test/ssr/test_deno_setters.rb`** → rename to `test_deno_config.rb`, test `Config` directly
10. **`test/ssr/test_deno_env_config.rb`** — update all assertions
11. **`test/ssr/test_deno_stability.rb`** — 2 calls
12. **`test/ssr/test_deno_render_timeout.rb`** — 6 calls
13. **`test/ssr/test_integration_deno_rails.rb`** — 3 assertions

### Signatures
14. **`sig/ssr/deno.rbs`** — add `Config` module, remove config methods from `SSR::Deno`

### Docs
15. **`README.md`** — update config snippets
16. **`docs/compatibility.md`** — update ref
17. **`docs/architecture.md`** — update ref

## Out of scope

- `CHANGELOG.md` — historical, skip
- `plans/archived/*` — historical, skip
- `heap_stats`/`heap_stats!` — stay on `SSR::Deno` (runtime ops, not config)

## Pattern

Search-replace across all files:
```
SSR::Deno.max_heap_size_mb  →  SSR::Deno::Config.max_heap_size_mb
SSR::Deno.isolate_pool_size →  SSR::Deno::Config.isolate_pool_size
SSR::Deno.render_timeout_ms →  SSR::Deno::Config.render_timeout_ms
SSR::Deno.node_builtins_enabled → SSR::Deno::Config.node_builtins_enabled
```

## Verification

1. `bundle exec ruby -e 'require_relative "lib/ssr/deno/config"; SSR::Deno::Config.max_heap_size_mb = 128; puts SSR::Deno::Config.max_heap_size_mb'` — quick smoke test
2. `bundle exec rake test` — full pipeline
3. `bundle exec rubocop` — lint
4. `bundle exec rake rbs` — type check
5. `rg 'SSR::Deno\.(max_heap_size_mb|isolate_pool_size|render_timeout_ms|node_builtins_enabled)[^_]' lib/ test/ rakelib/ scripts/ Dockerfile docs/ README.md` — verify no stale refs
