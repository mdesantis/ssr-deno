#!/usr/bin/env ruby
# frozen_string_literal: true

# ---------------------------------------------------------------------------
# ssr-deno Performance Regression Check (CI gate)
#
# Runs a fast subset of bench/performance.rb and checks:
#   1. Architecture invariants (ractor > single, thread ≈ single)
#   2. Absolute baselines (if config/perf-baselines.yml exists)
#
# Usage:
#   ruby bench/perf-check.rb                          # check against baselines
#   ruby bench/perf-check.rb --update-baseline         # regenerate baselines
#
# Exit code: 0 on pass, 1 on regression.
# ---------------------------------------------------------------------------

require 'json'
require 'yaml'
require 'fileutils'
require 'etc'

# ---------------------------------------------------------------------------
# Paths
# ---------------------------------------------------------------------------

ROOT = File.expand_path('..', __dir__).freeze
FIXTURES_DIR = File.join(ROOT, 'test', 'fixtures').freeze
TMP_DIR = File.join(ROOT, 'tmp').freeze
SAMPLES_DIR = File.join(ROOT, 'samples').freeze
CONFIG_DIR = File.join(ROOT, 'config').freeze
BASELINE_FILE = File.join(CONFIG_DIR, 'perf-baselines.yml').freeze

MINIMAL_BUNDLE = File.join(FIXTURES_DIR, 'minimal-bundle.js').freeze

def sample_bundle(name, subdir)
  path = File.join(TMP_DIR, "#{name}.js")
  src = File.join(SAMPLES_DIR, subdir, 'dist/server/entry-server.js')
  unless File.exist?(path)
    FileUtils.cp(src, path)
  end
  path
end

REACT_BUNDLE = sample_bundle('react-ssr', 'vite-react-ssr-app')
MUI_EMOTION_BUNDLE = sample_bundle('react-mui-emotion-ssr', 'vite-react-mui-emotion-ssr-app')

# ---------------------------------------------------------------------------
# Config
# ---------------------------------------------------------------------------

ITERS = {
  minimal: { render: 100, warmup: 20 },
  react: { render: 20, warmup: 10 },
  mui_emotion: { render: 10, warmup: 5 },
}.freeze

POOL_SIZE = 4
RACTOR_COUNT = 4
THREAD_COUNT = 4

UPDATE_BASELINE = ARGV.delete('--update-baseline')

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def fmt_ms(s)
  format('%.1f', s * 1000)
end

def fmt_ops(count, elapsed_s)
  return '0' if elapsed_s <= 0
  format('%d', count / elapsed_s)
end

def percentile(sorted, p)
  return 0.0 if sorted.empty?
  idx = [(p.to_f / 100) * sorted.size, sorted.size - 1].min
  sorted[idx.to_i]
end

def measure(label, pool_size, iterations, warmup)
  bundle_path = yield
  payload = { data: { name: 'perf' } }

  bundle = SSR::Deno::Bundle.new(bundle_path)
  warmup.times { bundle.render(payload) }

  # Single thread timing
  single_timings = []
  t0 = Process.clock_gettime(Process::CLOCK_MONOTONIC)
  iterations.times do
    tc = Process.clock_gettime(Process::CLOCK_MONOTONIC)
    bundle.render(payload)
    single_timings << (Process.clock_gettime(Process::CLOCK_MONOTONIC) - tc)
  end
  single_elapsed = Process.clock_gettime(Process::CLOCK_MONOTONIC) - t0
  single_sorted = single_timings.sort
  single_ops = fmt_ops(iterations, single_elapsed).to_i
  single_p50 = percentile(single_sorted, 50)
  single_p99 = percentile(single_sorted, 99)

  puts "  #{label} single-thread: #{iterations} renders in #{fmt_ms(single_elapsed)}ms" \
       " | #{single_ops} ops/sec | p50: #{fmt_ms(single_p50)}ms p99: #{fmt_ms(single_p99)}ms"

  # Multi-thread timing
  thread_timings = []
  t0 = Process.clock_gettime(Process::CLOCK_MONOTONIC)
  threads = Array.new(THREAD_COUNT) do |i|
    Thread.new do
      iterations.times do
        tc = Process.clock_gettime(Process::CLOCK_MONOTONIC)
        bundle.render({ data: { name: "T#{i}" } })
        thread_timings << (Process.clock_gettime(Process::CLOCK_MONOTONIC) - tc)
      end
    end
  end
  threads.each(&:join)
  thread_elapsed = Process.clock_gettime(Process::CLOCK_MONOTONIC) - t0
  thread_total = iterations * THREAD_COUNT
  thread_ops = fmt_ops(thread_total, thread_elapsed).to_i
  puts "  #{label} #{THREAD_COUNT}-thread: #{thread_total} renders in #{fmt_ms(thread_elapsed)}ms" \
       " | #{thread_ops} ops/sec"

  # Multi-Ractor timing
  t0 = Process.clock_gettime(Process::CLOCK_MONOTONIC)
  lib_path = File.join(ROOT, 'lib')
  ractors = Array.new(RACTOR_COUNT) do |i|
    Ractor.new(bundle_path, iterations, i, lib_path) do |path, iters, idx, lp|
      require 'json'
      require File.join(lp, 'ssr/deno')
      rb = SSR::Deno::Bundle.new(path)
      iters.times { rb.render({ data: { name: "R#{idx}" } }) }
    end
  end
  ractors.each(&:value)
  ractor_elapsed = Process.clock_gettime(Process::CLOCK_MONOTONIC) - t0
  ractor_total = iterations * RACTOR_COUNT
  ractor_ops = fmt_ops(ractor_total, ractor_elapsed).to_i
  puts "  #{label} #{RACTOR_COUNT}-ractor: #{ractor_total} renders in #{fmt_ms(ractor_elapsed)}ms" \
       " | #{ractor_ops} ops/sec"

  {
    label: label,
    pool_size: pool_size,
    iterations: iterations,
    single_ops: single_ops,
    single_p50_ms: (single_p50 * 1000).round(2),
    single_p99_ms: (single_p99 * 1000).round(2),
    thread_ops: thread_ops,
    ractor_ops: ractor_ops,
  }
end

# ---------------------------------------------------------------------------
# Invariant checks
# ---------------------------------------------------------------------------

def check_invariant(label, passed, detail)
  status = passed ? 'PASS' : 'FAIL'
  puts "  [#{status}] #{label}: #{detail}"
  passed
end

def check_invariants(results)
  all_pass = true

  min = results[:minimal]
  react = results[:react]
  mui = results[:mui_emotion]

  # Ractor mode must outperform single-thread (parallelism works).
  all_pass &= check_invariant(
    'ractor > single',
    min[:ractor_ops] > min[:single_ops],
    "Ractor #{min[:ractor_ops]} ops vs Single #{min[:single_ops]} ops",
  )

  # Thread mode must NOT outperform single-thread by much (GVL serializes).
  thread_ratio = min[:thread_ops].to_f / [min[:single_ops], 1].max
  all_pass &= check_invariant(
    'thread ≈ single',
    thread_ratio < 1.3,
    "Thread/single ratio: #{thread_ratio.round(2)} (threshold: < 1.3)",
  )

  # React is always slower than minimal.
  react_ratio = react[:single_ops].to_f / [min[:single_ops], 1].max
  all_pass &= check_invariant(
    'React < minimal',
    react_ratio < 0.5,
    "React/minimal ratio: #{react_ratio.round(2)} (threshold: < 0.5)",
  )

  # MUI emotion is always slower than React.
  mui_ratio = mui[:single_ops].to_f / [react[:single_ops], 1].max
  all_pass &= check_invariant(
    'MUI < React',
    mui_ratio < 0.5,
    "MUI/React ratio: #{mui_ratio.round(2)} (threshold: < 0.5)",
  )

  # No crashes during benchmark (any nil result indicates crash).
  results.each_value do |r|
    all_pass &= check_invariant(
      "no crash (#{r[:label]})",
      r[:single_ops] > 0,
      "ops: #{r[:single_ops]}",
    )
  end

  all_pass
end

# ---------------------------------------------------------------------------
# Baseline comparison
# ---------------------------------------------------------------------------

def load_baselines
  return nil unless File.exist?(BASELINE_FILE)
  YAML.safe_load_file(BASELINE_FILE)
end

def baseline_key(label, mode)
  "#{label}_#{mode}_ops"
end

def check_baselines(results)
  baselines = load_baselines
  unless baselines
    puts "\n  [SKIP] No baselines at #{BASELINE_FILE}. Run --update-baseline to create."
    return true
  end

  all_pass = true
  thresholds = baselines['thresholds'] || { 'ops_pct' => 70, 'p99_mult' => 5.0 }

  baselines['baselines'].each do |key, bl|
    # key format: "minimal_single" or "react_ractor"
    label, mode = key.split('_', 2)
    result = results.values.find { |r| r[:label] == label }
    next unless result

    current = result[:"#{mode}_ops"]
    next unless current

    pct = current.to_f / bl['ops'] * 100
    if pct < thresholds['ops_pct']
      puts "  [FAIL] #{key}: #{current} ops (#{pct.round(0)}% of baseline #{bl['ops']})"
      all_pass = false
    else
      puts "  [PASS] #{key}: #{current} ops (#{pct.round(0)}% of baseline #{bl['ops']})"
    end
  end

  all_pass
end

# ---------------------------------------------------------------------------
# Baseline update
# ---------------------------------------------------------------------------

def write_baselines(results)
  FileUtils.mkdir_p(CONFIG_DIR)

  bl_data = {}

  results.each_value do |r|
    %w[single thread ractor].each do |mode|
      key = "#{r[:label]}_#{mode}"
      val = r[:"#{mode}_ops"]
      bl_data[key] = { 'ops' => val } if val&.positive?
    end
  end

  baselines = {
    'meta' => {
      'generated' => Time.now.strftime('%Y-%m-%d'),
      'ruby' => RUBY_VERSION,
      'cpu' => Etc.nprocessors,
    },
    'thresholds' => { 'ops_pct' => 70, 'p99_mult' => 5.0 },
    'baselines' => bl_data,
  }

  File.write(BASELINE_FILE, YAML.dump(baselines))
  puts "\n  Baselines written to #{BASELINE_FILE}"
end

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

begin
  $LOAD_PATH.unshift File.join(ROOT, 'lib')
  require 'ssr/deno'

  SSR::Deno.isolate_pool_size = POOL_SIZE
  SSR::Deno.render_timeout_ms = 5000
  SSR::Deno.node_builtins_enabled = true

  puts "=" * 60
  puts "ssr-deno Performance Check"
  puts "Ruby: #{RUBY_VERSION} | pool: #{POOL_SIZE} | ractors: #{RACTOR_COUNT}"
  puts "=" * 60

  results = {}

  # --- Minimal bundle ---
  puts "\n--- minimal-bundle.js ---"
  results[:minimal] = measure('minimal', POOL_SIZE, ITERS[:minimal][:render], ITERS[:minimal][:warmup]) do
    MINIMAL_BUNDLE
  end

  # --- React SSR bundle ---
  puts "\n--- react-ssr-bundle.js ---"
  results[:react] = measure('react', POOL_SIZE, ITERS[:react][:render], ITERS[:react][:warmup]) do
    REACT_BUNDLE
  end

  # --- MUI emotion bundle (node_builtins) ---
  puts "\n--- react-mui-emotion-ssr-bundle.js ---"
  results[:mui_emotion] = measure('mui_emotion', POOL_SIZE, ITERS[:mui_emotion][:render], ITERS[:mui_emotion][:warmup]) do
    MUI_EMOTION_BUNDLE
  end

  # --- Checks ---
  puts "\n--- Invariants ---"
  invariants_pass = check_invariants(results)
  puts "\n--- Baselines ---"
  baselines_pass = check_baselines(results)

  if UPDATE_BASELINE
    write_baselines(results)
  end

  puts "\n" + "=" * 60
  if invariants_pass && baselines_pass
    puts "RESULT: PASS"
    exit 0
  else
    puts "RESULT: FAIL"
    exit 1
  end
rescue => e
  puts "\nRESULT: FAIL (exception)"
  puts "#{e.class}: #{e.message}"
  puts e.backtrace.first(3).join("\n")
  exit 1
end
