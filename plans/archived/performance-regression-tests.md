# Performance Regression Tests

Superseded by [minitest-perf-regression.md](minitest-perf-regression.md).

The Minitest-based implementation (`test/ssr/test_perf.rb`) replaced the
`bench/perf-check.rb` approach described here. Key differences:

- Uses `Minitest::Benchmark` with custom assertions instead of standalone script
- Single `bench_all` method avoids pool-state variance
- Runs as `test:perf` suite in `bundle exec rake`, not via separate script
- Baselines moved from `config/` to `test/fixtures/`
- No complexity order check (too noisy with small samples)
