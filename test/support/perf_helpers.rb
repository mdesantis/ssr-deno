# frozen_string_literal: true

# ---------------------------------------------------------------------------
# Standalone helpers
# ---------------------------------------------------------------------------

def percentile(sorted, pct)
  return 0.0 if sorted.empty?

  idx = [(pct.to_f / 100) * sorted.size, sorted.size - 1].min
  sorted[idx.to_i]
end

def fmt_ops(count, elapsed_s)
  return '0' if elapsed_s <= 0

  format('%d', count / elapsed_s)
end

def fmt_ms(secs)
  format('%.1f', secs * 1000)
end

# ---------------------------------------------------------------------------
# PerfHelpers — measurement functions
# ---------------------------------------------------------------------------

module PerfHelpers
  REACT_BUNDLE = File.expand_path('../../samples/vite-react-ssr-app/dist/server/entry-server.js', __dir__).freeze
  MUI_EMOTION_BUNDLE = File.expand_path('../../samples/vite-react-mui-emotion-ssr-app/dist/server/entry-server.js',
                                        __dir__).freeze

  def benchmark_single(bundle_path, iterations:, warmup: 20)
    payload = { data: { name: 'perf' } }

    bundle = SSR::Deno::Bundle.new(bundle_path)
    warmup.times { bundle.render(payload) }

    timings = []
    start_time = Process.clock_gettime(Process::CLOCK_MONOTONIC)
    iterations.times do
      tc = Process.clock_gettime(Process::CLOCK_MONOTONIC)
      bundle.render(payload)
      timings << (Process.clock_gettime(Process::CLOCK_MONOTONIC) - tc)
    end
    elapsed = Process.clock_gettime(Process::CLOCK_MONOTONIC) - start_time

    sorted = timings.sort
    ops = fmt_ops(iterations, elapsed).to_i
    p50_ms = (percentile(sorted, 50) * 1000).round(2)
    p99_ms = (percentile(sorted, 99) * 1000).round(2)

    puts "    #{iterations} renders in #{fmt_ms(elapsed)}ms " \
         "| #{ops} ops/sec | p50: #{fmt_ms(percentile(sorted, 50))}ms " \
         "p99: #{fmt_ms(percentile(sorted, 99))}ms"

    { ops: ops, p50_ms: p50_ms, p99_ms: p99_ms }
  end

  def benchmark_parallel(bundle_path, mode:, iterations:, count: 4)
    lib_path = File.expand_path('../../lib', __dir__)

    start_time = Process.clock_gettime(Process::CLOCK_MONOTONIC)

    case mode
    when :threads
      threads = Array.new(count) do |i|
        Thread.new do
          bundle = SSR::Deno::Bundle.new(bundle_path)
          iterations.times { bundle.render({ data: { name: "T#{i}" } }) }
        end
      end
      threads.each(&:join)
    when :ractors
      ractors = Array.new(count) do |i|
        Ractor.new(bundle_path, iterations, i, lib_path) do |path, iters, idx, lp|
          require 'json'
          require File.join(lp, 'ssr/deno')
          rb = SSR::Deno::Bundle.new(path)
          iters.times { rb.render({ data: { name: "R#{idx}" } }) }
        end
      end
      ractors.each(&:value)
    end

    elapsed = Process.clock_gettime(Process::CLOCK_MONOTONIC) - start_time
    total = iterations * count
    ops = fmt_ops(total, elapsed).to_i

    puts "    #{total} renders in #{fmt_ms(elapsed)}ms " \
         "| #{ops} ops/sec"

    { ops: ops, total_renders: total, elapsed_ms: (elapsed * 1000).round(1) }
  end

  def benchmark_ractor_pool(bundle_path, iterations:, size: 4, warmup: 20)
    payload = { data: { name: 'perf' } }

    pool = SSR::Deno::RactorPool.new(bundle_path:, size:)
    warmup.times { pool.render(payload) }

    timings = []
    start_time = Process.clock_gettime(Process::CLOCK_MONOTONIC)
    iterations.times do
      tc = Process.clock_gettime(Process::CLOCK_MONOTONIC)
      pool.render(payload)
      timings << (Process.clock_gettime(Process::CLOCK_MONOTONIC) - tc)
    end
    elapsed = Process.clock_gettime(Process::CLOCK_MONOTONIC) - start_time

    sorted = timings.sort
    ops = fmt_ops(iterations, elapsed).to_i
    p50_ms = (percentile(sorted, 50) * 1000).round(2)
    p99_ms = (percentile(sorted, 99) * 1000).round(2)

    puts "    #{iterations} renders in #{fmt_ms(elapsed)}ms " \
         "| #{ops} ops/sec | p50: #{fmt_ms(percentile(sorted, 50))}ms " \
         "p99: #{fmt_ms(percentile(sorted, 99))}ms"

    { ops: ops, p50_ms: p50_ms, p99_ms: p99_ms }
  end
end

# ---------------------------------------------------------------------------
# PerfAssertions — custom Minitest assertions
# ---------------------------------------------------------------------------

module PerfAssertions
  RACTOR_SPEEDUP_FACTOR = 1.5
  THREAD_PARALLEL_FACTOR = 1.3

  def assert_ractor_faster(single_ops, ractor_ops, factor: RACTOR_SPEEDUP_FACTOR)
    threshold = single_ops * factor

    assert_operator ractor_ops, :>, threshold,
                    "Expected Ractor ops (#{ractor_ops}) > #{factor}x Single ops (#{single_ops})"
  end

  def assert_thread_parallel(single_ops, thread_ops, min_factor: THREAD_PARALLEL_FACTOR)
    ratio = thread_ops.to_f / [single_ops, 1].max

    assert_operator ratio, :>, min_factor,
                    "Thread/single ratio #{ratio.round(2)} is below #{min_factor} " \
                    '(GVL release should enable parallel FFI)'
  end

  def assert_bundle_heavier(fast_ops, slow_ops, fast_label, slow_label)
    assert_operator fast_ops, :>, slow_ops,
                    "Expected #{fast_label} (#{fast_ops} ops) > #{slow_label} (#{slow_ops} ops) " \
                    '— bundle complexity order violated'
  end

  def assert_no_crash(ops, label)
    assert_operator ops, :>, 0, "#{label}: 0 ops (render likely crashed)"
  end

  # A committed absolute-ops baseline doesn't transfer across machines: the
  # same dependency set measured ~9.7k ops/sec for minimal_single on a
  # 24-core box and ~20k on a 10-core laptop — a >2x swing from hardware
  # alone, before any real regression. Cross-bundle ratios (react_single as
  # a fraction of minimal_single) swing even harder, since trivial and heavy
  # bundles are bottlenecked on different things (FFI/dispatch overhead vs.
  # actual V8 execution) that don't scale together across CPU generations.
  #
  # What *does* stay in the same order of magnitude across machines: the
  # same bundle's own single-render vs. RactorPool-render ratio, measured in
  # the same run. This asserts that ratio doesn't collapse — RactorPool
  # dispatch overhead becoming catastrophically worse relative to single
  # mode for the same workload — without needing any committed number.
  # `min_ratio` is intentionally generous (observed 44-110% across two very
  # different machines); this is a floor against a real regression, not a
  # performance target.
  def assert_pool_overhead_reasonable(single_ops, pool_ops, label, min_ratio: 0.2)
    ratio = pool_ops.to_f / [single_ops, 1].max

    assert_operator ratio, :>=, min_ratio,
                    "#{label}: RactorPool/single ratio #{ratio.round(2)} is below #{min_ratio} " \
                    "(#{pool_ops} pool ops vs #{single_ops} single ops for the same bundle)"
  end
end
