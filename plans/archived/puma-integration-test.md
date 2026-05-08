# Plan: Puma Integration Test

## Context

Puma clustered mode (fork-based) has a known V8 limitation: isolates
cannot be created after fork. Correct strategy is to defer `Bundle.new`
to `on_worker_boot` (or lazy-init inside `call`). We need integration
tests that verify the working configurations and keep coverage.

## Tasks

- [x] Add `puma` to Gemfile, bundle install
- [x] Create `test/dummy/config_puma.ru` — Rack app with lazy Bundle
- [x] Create `test/dummy/puma_single.rb` — single mode Puma config
- [x] Create `test/dummy/puma_clustered_on_worker_boot.rb` — workers 2 config
- [x] Write `test/ssr/test_integration_puma.rb` — single mode (in-process,
      coverage-tracked) + clustered mode (subprocess)
- [x] Add `test:puma` to `rakelib/test.rake` (SIMPLECOV_COMMAND_NAME,
      render_timeout_ms=5000)
- [x] Add `test:puma` to `test` dependency list
- [x] Stale audit: Rakefile comment, CHANGELOG, CI config

## Verification

- `bundle exec rake` — full pipeline exits 0
- `bundle exec rake test:puma` — standalone passes
- Coverage at 100% line + 100% branch
