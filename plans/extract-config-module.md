# Extract SSR::Deno::Config module

Move config setters/getters/env-defaults out of `SSR::Deno` into dedicated `SSR::Deno::Config` module. Native FFI methods stay on `SSR::Deno`.

## Status

✅ Complete — committed at 8faae65.

## Files created

- `lib/ssr/deno/config.rb`

## Files modified

- `lib/ssr/deno.rb` — stripped config methods, added require + apply_env_var_defaults
- `lib/ssr/deno/rails/railtie.rb` — `SSR::Deno.` → `SSR::Deno::Config.`
- `lib/ssr/deno/ractor_pool.rb` — doc comment
- `rakelib/test.rake` — 11 calls + task rename setters→config
- `rakelib/perf.rake` — 3 calls
- `scripts/throughput.rb` — 3 calls
- `scripts/performance.rb` — 3 calls
- `Dockerfile` — 2 lines
- `test/ssr/test_deno_setters.rb` → `test_deno_config.rb`
- `test/ssr/test_deno_env_config.rb`
- `test/ssr/test_deno_stability.rb`
- `test/ssr/test_deno_render_timeout.rb`
- `test/ssr/test_integration_deno_rails.rb`
- `test/ssr/test_integration_samples.rb`
- `sig/ssr/deno.rbs` — added Config module
- `README.md`
- `docs/compatibility.md`
- `docs/architecture.md`
- `Rakefile`
- `plans/attachments/reproduce_v8_oom.rb`

## Verification

- ✅ `bundle exec rake` — all tests 0 failures, coverage 100/100
- ✅ RuboCop — 0 offenses
- ✅ RBS — validated
