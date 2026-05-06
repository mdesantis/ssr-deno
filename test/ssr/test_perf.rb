# frozen_string_literal: true

require 'test_helper'
require 'minitest/benchmark'
require 'support/perf_helpers'

module SSR
  class PerfRegression < Minitest::Benchmark
    include PerfHelpers
    include PerfAssertions
    include TestFixturePaths

    MINIMAL = TestFixturePaths::MINIMAL_BUNDLE

    # Single combined benchmark — shared pool warmup across all bundles,
    # all invariants and baseline checks in one method. Avoids pool-state
    # variance from multiple bench_ methods loading bundles in different
    # orders.
    def bench_all
      # --- Minimal bundle ---
      min_single = benchmark_single(MINIMAL, iterations: 100, warmup: 20)
      min_ractor = benchmark_parallel(MINIMAL, mode: :ractors, iterations: 100)
      min_threads = benchmark_parallel(MINIMAL, mode: :threads, iterations: 100)

      assert_no_crash(min_single[:ops], 'minimal single')
      assert_no_crash(min_ractor[:ops], 'minimal ractor')
      assert_no_crash(min_threads[:ops], 'minimal threads')
      assert_ractor_faster(min_single[:ops], min_ractor[:ops])
      assert_thread_not_parallel(min_single[:ops], min_threads[:ops])

      # --- React SSR bundle ---
      react_single = benchmark_single(REACT_BUNDLE, iterations: 50, warmup: 10)
      react_ractor = benchmark_parallel(REACT_BUNDLE, mode: :ractors, iterations: 50)

      assert_no_crash(react_single[:ops], 'react single')
      assert_no_crash(react_ractor[:ops], 'react ractor')
      assert_ractor_faster(react_single[:ops], react_ractor[:ops])

      # --- MUI emotion bundle (node_builtins) ---
      mui_single = benchmark_single(MUI_EMOTION_BUNDLE, iterations: 20, warmup: 5)

      assert_no_crash(mui_single[:ops], 'MUI emotion single')

      # --- Baseline checks (optional) ---
      assert_within_baseline(min_single[:ops], 'minimal_single')
      assert_within_baseline(react_single[:ops], 'react_single', pct: 50)
      assert_within_baseline(mui_single[:ops], 'mui_emotion_single', pct: 50)
    end
  end
end
