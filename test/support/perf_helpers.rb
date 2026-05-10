# frozen_string_literal: true

require 'yaml'
require 'json'
require 'etc'

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

  # -------------------------------------------------------------------------
  # Baseline I/O
  # -------------------------------------------------------------------------

  BASELINE_FILE = File.expand_path('../../test/fixtures/perf-baselines.yml', __dir__).freeze

  def self.load_baselines
    return nil unless File.exist?(BASELINE_FILE)

    YAML.safe_load_file(BASELINE_FILE)
  end

  def self.write_baselines(results, path = BASELINE_FILE)
    FileUtils.mkdir_p(File.dirname(path))

    bl_data = {}
    results.each do |key, ops|
      bl_data[key.to_s] = { 'ops' => ops }
    end

    baselines = {
      'meta' => {
        'generated' => Time.now.strftime('%Y-%m-%d'),
        'ruby' => RUBY_VERSION,
        'cpu' => Etc.nprocessors
      },
      'thresholds' => { 'ops_pct' => 70, 'p99_mult' => 5.0 },
      'baselines' => bl_data
    }

    File.write(path, YAML.dump(baselines))
  end
end

# ---------------------------------------------------------------------------
# PerfAssertions — custom Minitest assertions
# ---------------------------------------------------------------------------

module PerfAssertions
  def assert_ractor_faster(single_ops, ractor_ops, factor: 1.5)
    threshold = single_ops * factor

    assert_operator ractor_ops, :>, threshold,
                    "Expected Ractor ops (#{ractor_ops}) > #{factor}x Single ops (#{single_ops})"
  end

  def assert_thread_not_parallel(single_ops, thread_ops, tolerance: 0.3)
    ratio = thread_ops.to_f / [single_ops, 1].max

    assert_operator ratio, :<, 1.0 + tolerance,
                    "Thread/single ratio #{ratio.round(2)} exceeds #{1.0 + tolerance} " \
                    '(GVL should serialize FFI)'
  end

  def assert_bundle_heavier(fast_ops, slow_ops, fast_label, slow_label)
    assert_operator fast_ops, :>, slow_ops,
                    "Expected #{fast_label} (#{fast_ops} ops) > #{slow_label} (#{slow_ops} ops) " \
                    '— bundle complexity order violated'
  end

  def assert_no_crash(ops, label)
    assert_operator ops, :>, 0, "#{label}: 0 ops (render likely crashed)"
  end

  def assert_within_baseline(ops, key, pct: 70)
    baselines = PerfHelpers.load_baselines
    skip 'No baselines at test/fixtures/perf-baselines.yml — run rake perf:baseline:update' unless baselines

    baseline = baselines['baselines'][key.to_s]
    skip "No baseline entry for #{key}" unless baseline

    threshold = baseline['ops'] * pct / 100.0

    assert_operator ops, :>=, threshold.round,
                    "#{key}: #{ops} ops is below #{pct}% of baseline (#{baseline['ops']} ops)"
  end
end
