# frozen_string_literal: true

require 'test_helper'
require 'support/perf_helpers'

module SSR
  class PerfRegression < Minitest::Test
    include PerfHelpers
    include PerfAssertions
    include TestFixturePaths

    MINIMAL = TestFixturePaths::MINIMAL_BUNDLE

    class << self
      # rubocop:disable ThreadSafety/ClassInstanceVariable
      def run_benchmarks
        return if @benchmark_results

        inst = allocate
        r = {}

        puts "\n=== Running all benchmarks (single pass) ==="

        r[:min_single] = inst.benchmark_single(MINIMAL, iterations: 100, warmup: 20)
        r[:min_ractor] = inst.benchmark_parallel(MINIMAL, mode: :ractors, iterations: 100)
        r[:min_threads] = inst.benchmark_parallel(MINIMAL, mode: :threads, iterations: 100)
        r[:min_ractor_pool] = inst.benchmark_ractor_pool(MINIMAL, iterations: 100, warmup: 20)

        r[:react_single] = inst.benchmark_single(PerfHelpers::REACT_BUNDLE, iterations: 50, warmup: 10)
        r[:react_ractor] = inst.benchmark_parallel(PerfHelpers::REACT_BUNDLE, mode: :ractors, iterations: 50)

        r[:mui_single] = inst.benchmark_single(PerfHelpers::MUI_EMOTION_BUNDLE, iterations: 20, warmup: 5)
        r[:mui_ractor_pool] = inst.benchmark_ractor_pool(PerfHelpers::MUI_EMOTION_BUNDLE, iterations: 20, warmup: 5)

        @benchmark_results = r
      end

      attr_reader :benchmark_results
      # rubocop:enable ThreadSafety/ClassInstanceVariable
    end

    def before_setup
      self.class.run_benchmarks
    end

    def r
      self.class.benchmark_results
    end

    def test_minimal_no_crash
      assert_no_crash(r[:min_single][:ops], 'minimal single')
      assert_no_crash(r[:min_ractor][:ops], 'minimal ractor')
      assert_no_crash(r[:min_threads][:ops], 'minimal threads')
      assert_no_crash(r[:min_ractor_pool][:ops], 'minimal ractor_pool')
    end

    def test_minimal_parallel_speedup
      assert_ractor_faster(r[:min_single][:ops], r[:min_ractor][:ops])
      assert_thread_parallel(r[:min_single][:ops], r[:min_threads][:ops])
    end

    def test_react_no_crash
      assert_no_crash(r[:react_single][:ops], 'react single')
      assert_no_crash(r[:react_ractor][:ops], 'react ractor')
    end

    def test_react_ractor_speedup
      assert_ractor_faster(r[:react_single][:ops], r[:react_ractor][:ops])
    end

    def test_mui_emotion_no_crash
      assert_no_crash(r[:mui_single][:ops], 'MUI emotion single')
      assert_no_crash(r[:mui_ractor_pool][:ops], 'MUI emotion ractor_pool')
    end

    def test_baseline_checks
      assert_within_baseline(r[:min_single][:ops], 'minimal_single')
      assert_within_baseline(r[:min_ractor_pool][:ops], 'minimal_ractor_pool')
      assert_within_baseline(r[:react_single][:ops], 'react_single', pct: 50)
      assert_within_baseline(r[:mui_single][:ops], 'mui_emotion_single', pct: 50)
      assert_within_baseline(r[:mui_ractor_pool][:ops], 'mui_emotion_ractor_pool', pct: 50)
    end
  end
end
