#!/usr/bin/env ruby
# frozen_string_literal: true

# ---------------------------------------------------------------------------
# ssr-deno Performance Benchmark — PoC
#
# Measures render throughput across three concurrency patterns:
#   1. Single thread (baseline sequential)
#   2. Multi-thread (shared GVL, round-robin pool dispatch)
#   3. Multi-Ractor (no GVL, true parallelism)
#
# Bundles are auto-built from samples/ on first use. Build outputs land in tmp/
# (gitignored). Requires: `deno task build` prerequisites (see samples/).
#
# Usage:
#   ruby bench/performance.rb                                # vite-react-ssr-app, pool=1, single
#   ruby bench/performance.rb --sample vite-react-ssr-app    # explicit sample
#   ruby bench/performance.rb --bundle minimal               # using alias
#   ruby bench/performance.rb --pool-size 4 --mode threads   # different config
#   ruby bench/performance.rb --sample vite-svelte-ssr-app   # different sample
#
# Requires: compiled native extension (bundle exec rake compile)
# ---------------------------------------------------------------------------

require 'json'
require 'optparse'
require 'etc'
require 'fileutils'

Warning[:experimental] = false

# ---------------------------------------------------------------------------
# Paths
# ---------------------------------------------------------------------------

BENCH_ROOT = File.expand_path('..', __dir__).freeze
FIXTURES_DIR = File.join(BENCH_ROOT, 'test', 'fixtures').freeze
TMP_DIR = File.join(BENCH_ROOT, 'tmp').freeze
MINIMAL_BUNDLE = File.join(FIXTURES_DIR, 'minimal-bundle.js').freeze
REACT_BUNDLE = File.join(TMP_DIR, 'react-ssr-bundle.js').freeze
MUI_EMOTION_BUNDLE = File.join(TMP_DIR, 'react-mui-emotion-ssr-bundle.js').freeze
MUI_DASHBOARD_BUNDLE = File.join(TMP_DIR, 'react-mui-dashboard-ssr-bundle.js').freeze
SAMPLES_DIR = File.join(BENCH_ROOT, 'samples').freeze

# ---------------------------------------------------------------------------
# Config
# ---------------------------------------------------------------------------

BUNDLE_ALIASES = {
  'minimal' => MINIMAL_BUNDLE,
  'react' => REACT_BUNDLE,
  'mui-emotion' => MUI_EMOTION_BUNDLE,
  'mui-dashboard' => MUI_DASHBOARD_BUNDLE,
}.freeze

options = {
  iterations: 200,
  warmup: 10,
  thread_count: 4,
  ractor_count: 4,
  pool_size: 1,
  mode: :single,
  bundle: nil,
  sample: nil,
  node_builtins: false,
  timeout_ms: nil,
}

OptionParser.new do |opts|
  opts.banner = "Usage: #{$PROGRAM_NAME} [options]"

  opts.on('-n', '--iterations N', Integer, 'Iterations per worker (default: 1000)') do |n|
    options[:iterations] = n
  end

  opts.on('-w', '--warmup N', Integer, 'Warmup iterations (default: 50)') do |n|
    options[:warmup] = n
  end

  opts.on('-t', '--threads N', Integer, 'Thread count (default: 4)') do |n|
    options[:thread_count] = n
  end

  opts.on('-r', '--ractors N', Integer, 'Ractor count (default: 4)') do |n|
    options[:ractor_count] = n
  end

  opts.on('-p', '--pool-size N', Integer, 'Isolate pool size (default: 1)') do |n|
    options[:pool_size] = n
  end

  opts.on('--mode MODE', %w[single threads ractors], 'Run only one mode') do |m|
    options[:mode] = m.to_sym
  end

  opts.on('--node-builtins', 'Enable Node.js built-in modules') do
    options[:node_builtins] = true
  end

  opts.on('--timeout MS', Integer, 'Render timeout in ms (default: 500)') do |ms|
    options[:timeout_ms] = ms
  end

  opts.on('--bundle NAME',
          "Bundle: #{BUNDLE_ALIASES.keys.join(' / ')} or file path") do |b|
    options[:bundle] = b
  end

  opts.on('--sample NAME', "Sample directory under samples/ (e.g. vite-react-ssr-app)") do |s|
    options[:sample] = s
  end
end.parse!

# ---------------------------------------------------------------------------
# Bundle resolution: --sample takes precedence, then --bundle, then default
# ---------------------------------------------------------------------------

if options[:sample]
  sample_dir = File.join(SAMPLES_DIR, options[:sample])
  bundle_path = File.join(sample_dir, 'dist/server/entry-server.js')

  unless File.exist?(sample_dir)
    abort "Sample not found: #{options[:sample]}. Available: #{Dir.glob("#{SAMPLES_DIR}/*/").map { |d| File.basename(d) }.sort.join(', ')}"
  end

  unless File.exist?(bundle_path)
    puts "  Building #{options[:sample]}..."
    success = system('deno', 'task', 'build', chdir: sample_dir)
    abort "Build failed for #{options[:sample]}" unless success
    abort "#{bundle_path} not found after build" unless File.exist?(bundle_path)
  end

  unless options[:node_builtins]
    options[:node_builtins] = File.read(bundle_path).match?(/(__)?require\(["'](stream|buffer|events|async_hooks|util)["']\)/)
  end

  options[:bundle] = bundle_path
elsif options[:bundle].nil?
  # Default: use vite-react-ssr-app sample
  options[:sample] = 'vite-react-ssr-app'
  sample_dir = File.join(SAMPLES_DIR, 'vite-react-ssr-app')
  bundle_path = File.join(sample_dir, 'dist/server/entry-server.js')

  unless File.exist?(bundle_path)
    puts "  Building default sample (vite-react-ssr-app)..."
    success = system('deno', 'task', 'build', chdir: sample_dir)
    abort "Build failed for default sample" unless success
  end

  options[:bundle] = bundle_path
end

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def percentile(sorted, p)
  return 0.0 if sorted.empty?
  idx = [(p.to_f / 100) * sorted.size, sorted.size - 1].min
  sorted[idx.to_i]
end

def fmt_ops(iterations, elapsed_s)
  return '0' if elapsed_s <= 0
  format('%d', iterations / elapsed_s)
end

def fmt_ms(s)
  format('%.1f', s * 1000)
end

def fmt_bytes(b)
  return "#{b} B" if b < 1024
  return "#{(b / 1024.0).round(1)} KB" if b < 1024 * 1024
  "#{(b / (1024.0 * 1024.0)).round(1)} MB"
end

def resolve_pool_label(size)
  size == 0 ? 'auto' : size.to_s
end

# ---------------------------------------------------------------------------
# Benchmark runner (single pool config)
# ---------------------------------------------------------------------------

def run_single_config(options)
  pool_size = options[:pool_size]
  iterations = options[:iterations]
  warmup = options[:warmup]
  thread_count = options[:thread_count]
  ractor_count = options[:ractor_count]
  mode_filter = options[:mode]
  bundle_path = BUNDLE_ALIASES[options[:bundle]] || options[:bundle]

  # Enable node builtins before loading ssr/deno (must happen before pool init).
  node_builtins = options[:node_builtins]

  # Set pool size before loading ssr/deno (triggers pool init).
  ENV['SSR_DENO_ISOLATE_POOL_SIZE'] = pool_size.to_s if pool_size > 0

  $LOAD_PATH.unshift File.join(BENCH_ROOT, 'lib')
  require 'ssr/deno'

  SSR::Deno.render_timeout_ms = options[:timeout_ms] if options[:timeout_ms]
  SSR::Deno.node_builtins_enabled = true if node_builtins

  payload = { data: { name: 'benchmark' } }

  # Warmup: initialize pool and let V8 reach steady state.
  bundle = SSR::Deno::Bundle.new(bundle_path)
  warmup.times { bundle.render(payload) }

  bundle_label = options[:bundle]
  initial_heap = SSR::Deno.heap_stats['used_heap_size']
  configured_pool = SSR::Deno.isolate_pool_size
  # Match Rust's resolve_pool_size: 0 = auto-detect, else clamp to [1, 8].
  actual_pool = if configured_pool == 0
    (Etc.nprocessors - 1).clamp(1, 8)
  else
    configured_pool.clamp(1, 8)
  end

  puts
  puts "=" * 60
  puts "ssr-deno Performance Benchmark"
  puts
  puts "Ruby version: #{RUBY_VERSION}"
  puts "SSR::Deno version: #{SSR::Deno.native_version}"
  puts "bundle: #{bundle_path}"
  puts "Pool size: #{resolve_pool_label(pool_size)}"
  mode_label = mode_filter ? mode_filter.to_s : 'all'
  puts "Mode: #{mode_label}"
  puts "Iterations: #{iterations}"
  puts "Warm: #{warmup}"
  puts "Timeout: #{options[:timeout_ms]}ms" if options[:timeout_ms]
  puts "=" * 60
  puts
  puts "  Heap: #{fmt_bytes(initial_heap)}"

  # ----- Mode 1: Single Thread -----
  if !mode_filter || mode_filter == :single
    timings = []

    t0 = Process.clock_gettime(Process::CLOCK_MONOTONIC)
    iterations.times do
      tc = Process.clock_gettime(Process::CLOCK_MONOTONIC)
      bundle.render(payload)
      timings << (Process.clock_gettime(Process::CLOCK_MONOTONIC) - tc)
    end
    elapsed = Process.clock_gettime(Process::CLOCK_MONOTONIC) - t0

    sorted = timings.sort
    puts "  Single Thread:"
    puts "    #{iterations} renders in #{fmt_ms(elapsed)}ms" \
         " | #{fmt_ops(iterations, elapsed)} ops/sec" \
         " | p50: #{fmt_ms(percentile(sorted, 50))}ms" \
         " p99: #{fmt_ms(percentile(sorted, 99))}ms"
    puts
  end

  # ----- Mode 2: Multi-Thread -----
  if !mode_filter || mode_filter == :threads
    timings = []

    t0 = Process.clock_gettime(Process::CLOCK_MONOTONIC)
    threads = Array.new(thread_count) do |i|
      Thread.new do
        iterations.times do
          tc = Process.clock_gettime(Process::CLOCK_MONOTONIC)
          bundle.render({ data: { name: "T#{i}" } })
          timings << (Process.clock_gettime(Process::CLOCK_MONOTONIC) - tc)
        end
      end
    end
    threads.each(&:join)
    elapsed = Process.clock_gettime(Process::CLOCK_MONOTONIC) - t0

    total = iterations * thread_count
    puts "  Multi-Thread (#{thread_count} threads):"
    puts "    #{total} renders in #{fmt_ms(elapsed)}ms" \
         " | #{fmt_ops(total, elapsed)} ops/sec"
    if actual_pool == 1
      puts "    bottleneck: single isolate"
    else
      puts "    (threads serialize on GVL during FFI —" \
           " see Multi-Ractor below for true parallelism)"
    end
    puts
  end

  # ----- Mode 3: Multi-Ractor -----
  if !mode_filter || mode_filter == :ractors
    t0 = Process.clock_gettime(Process::CLOCK_MONOTONIC)

    # Each Ractor creates its own Bundle instance — Ractors cannot share
    # mutable Ruby objects, but they can access shareable constants like
    # SSR::Deno::Bundle. The IsolatePool (Rust OnceLock) is process-shared,
    # so all Ractors dispatch through the same pool via round-robin.
    # The native extension declares rb_ext_ractor_safe(true) for safety.
    lib_path = File.join(BENCH_ROOT, 'lib')
    ractors = Array.new(ractor_count) do |i|
      Ractor.new(bundle_path, iterations, i, lib_path) do |path, iters, idx, lp|
        require 'json'
        require File.join(lp, 'ssr/deno')

        rb = SSR::Deno::Bundle.new(path)
        iters.times { rb.render({ data: { name: "R#{idx}" } }) }
      end
    end
    ractors.each(&:value)
    elapsed = Process.clock_gettime(Process::CLOCK_MONOTONIC) - t0

    total = iterations * ractor_count
    puts "  Multi-Ractor (#{ractor_count} Ractors):"
    puts "    #{total} renders in #{fmt_ms(elapsed)}ms" \
         " | #{fmt_ops(total, elapsed)} ops/sec | no GVL contention"
    puts
  end

  # ----- Memory Check -----
  final_heap = SSR::Deno.heap_stats['used_heap_size']
  delta = final_heap - initial_heap
  puts "  Memory: #{fmt_bytes(initial_heap)} → #{fmt_bytes(final_heap)}" \
       " (#{delta > 0 ? '+' : ''}#{fmt_bytes(delta.abs)})"
  puts
end

# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------

begin
  run_single_config(options)
rescue => e
  abort "Benchmark failed: #{e.message}\n#{e.backtrace.first(5).join("\n")}"
end
