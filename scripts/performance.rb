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
#   ruby scripts/performance.rb --sample vite-react-ssr-app
#   ruby scripts/performance.rb --sample vite-svelte-ssr-app
#   ruby scripts/performance.rb --sample vite-react-ssr-app --pool-size 4 --mode threads
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
SAMPLES_DIR = File.join(BENCH_ROOT, 'samples').freeze

# ---------------------------------------------------------------------------
# Config
# ---------------------------------------------------------------------------

options = {
  iterations: 200,
  warmup: 10,
  thread_count: nil,
  ractor_count: nil,
  pool_size: 1,
  mode: nil,
  sample: nil,
  timeout_ms: nil
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

  opts.on('--mode MODE', %w[single threads ractors ractor_pool all], 'Concurrency mode (default: single)') do |m|
    options[:mode] = m.to_sym
  end

  opts.on('--timeout MS', Integer, 'Render timeout in ms (default: 500)') do |ms|
    options[:timeout_ms] = ms
  end

  opts.on('--sample NAME', 'Sample directory under samples/ (e.g. vite-react-ssr-app)') do |s|
    options[:sample] = s
  end

  opts.on('--node-builtins', 'Enable Node.js builtin polyfills (default: auto-detect)') do
    options[:node_builtins] = true
  end

  opts.on('--no-node-builtins', 'Disable Node.js builtin polyfills') do
    options[:node_builtins] = false
  end
end.parse!

# ---------------------------------------------------------------------------
# Mode inference: --threads implies mode=threads, --ractors implies mode=ractors
# ---------------------------------------------------------------------------

options[:mode] ||= if options[:thread_count] && options[:ractor_count]
                     :single # both flags given, ambiguous — default to single
                   elsif options[:thread_count]
                     :threads
                   elsif options[:ractor_count]
                     :ractors
                   else
                     :single # no concurrency flag — sequential single-threaded
                   end

options[:thread_count] ||= 4
options[:ractor_count] ||= 4

# ---------------------------------------------------------------------------
# Bundle resolution: --sample required
# ---------------------------------------------------------------------------

sample = options[:sample]
unless sample
  abort "Usage: #{$PROGRAM_NAME} --sample <name>\n" \
        "Available samples: #{Dir.glob("#{SAMPLES_DIR}/*/").map { |d| File.basename(d) }.sort.join(', ')}"
end
sample_dir = File.join(SAMPLES_DIR, sample)
bundle_path = File.join(sample_dir, 'dist/server/entry-server.js')

unless File.exist?(sample_dir)
  abort "Sample not found: #{sample}. Available: #{Dir.glob("#{SAMPLES_DIR}/*/").map do |d|
    File.basename(d)
  end.sort.join(', ')}"
end

unless File.exist?(bundle_path)
  puts "  Building #{sample}..."
  success = system('deno', 'task', 'build', chdir: sample_dir)
  abort "Build failed for #{sample}" unless success
  abort "#{bundle_path} not found after build" unless File.exist?(bundle_path)
end

options[:bundle] = bundle_path

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def percentile(sorted, p)
  return 0.0 if sorted.empty?

  idx = (((p.to_f / 100) * sorted.size).ceil - 1).clamp(0, sorted.size - 1)
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
  size.zero? ? 'auto' : size.to_s
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
  bundle_path = options[:bundle]

  # Set pool size before loading ssr/deno (triggers pool init).
  ENV['SSR_DENO_ISOLATE_POOL_SIZE'] = pool_size.to_s if pool_size.positive?

  $LOAD_PATH.unshift File.join(BENCH_ROOT, 'lib')
  require 'ssr/deno'

  SSR::Deno.render_timeout_ms = options[:timeout_ms] if options[:timeout_ms]

  # Auto-infer node_builtins from bundle content.
  # Heuristic: scans for CommonJS require() of known Node.js builtins.
  # Misses: require('node:stream'), import ... from 'stream', dynamic
  # require(varName). Use --node-builtins / --no-node-builtins to override.
  if options.key?(:node_builtins)
    SSR::Deno.node_builtins_enabled = options[:node_builtins]
  elsif File.read(bundle_path).match?(/(__)?require\(["'](stream|buffer|events|async_hooks|util)["']\)/)
    SSR::Deno.node_builtins_enabled = true
  end

  payload = { data: { name: 'benchmark' } }

  # Warmup: initialize pool and let V8 reach steady state.
  use_bundle = %i[single threads ractors all].include?(mode_filter)
  use_pool = %i[ractor_pool all].include?(mode_filter)
  pool = nil

  if use_bundle
    bundle = SSR::Deno::Bundle.new(bundle_path)
    warmup.times { bundle.render(payload) }
  end

  if use_pool
    pool = SSR::Deno::RactorPool.new(bundle_path:, size: pool_size)
    warmup.times { pool.render(payload) }
  end

  initial_heap = SSR::Deno.heap_stats['used_heap_size']

  puts
  puts '=' * 60
  puts 'ssr-deno Performance Benchmark'
  puts
  puts "Ruby version: #{RUBY_VERSION}"
  puts "SSR::Deno version: #{SSR::Deno.native_version}"
  puts "bundle: #{bundle_path}"
  puts "Pool size: #{resolve_pool_label(pool_size)}"
  mode_label = mode_filter.to_s
  puts "Mode: #{mode_label}"
  puts "Threads: #{thread_count}" if %w[threads all].include?(mode_label)
  puts "Ractors: #{ractor_count}" if %w[ractors all].include?(mode_label)
  puts "RactorPool workers: #{pool_size}" if %w[ractor_pool all].include?(mode_label)
  puts "Iterations: #{iterations}"
  puts "Warm: #{warmup}"
  puts "Timeout: #{options[:timeout_ms]}ms" if options[:timeout_ms]
  puts '=' * 60
  puts
  puts "  Heap: #{fmt_bytes(initial_heap)}"

  # ----- Mode 1: Single Thread -----
  if %i[single all].include?(mode_filter)
    timings = []

    t0 = Process.clock_gettime(Process::CLOCK_MONOTONIC)
    iterations.times do
      tc = Process.clock_gettime(Process::CLOCK_MONOTONIC)
      bundle.render(payload)
      timings << (Process.clock_gettime(Process::CLOCK_MONOTONIC) - tc)
    end
    elapsed = Process.clock_gettime(Process::CLOCK_MONOTONIC) - t0

    sorted = timings.sort
    puts '  Single Thread:'
    puts "    #{iterations} renders in #{fmt_ms(elapsed)}ms " \
         "| #{fmt_ops(iterations, elapsed)} ops/sec " \
         "| p50: #{fmt_ms(percentile(sorted, 50))}ms " \
         "p99: #{fmt_ms(percentile(sorted, 99))}ms"
    puts
  end

  # ----- Mode 2: Multi-Thread -----
  if %i[threads all].include?(mode_filter)
    per_thread = iterations / thread_count
    extra = iterations % thread_count

    t0 = Process.clock_gettime(Process::CLOCK_MONOTONIC)
    threads = Array.new(thread_count) do |i|
      count = per_thread + (i < extra ? 1 : 0)
      Thread.new do
        count.times do
          bundle.render({ data: { name: "T#{i}" } })
        end
      end
    end
    threads.each(&:join)
    elapsed = Process.clock_gettime(Process::CLOCK_MONOTONIC) - t0

    puts "  Multi-Thread (#{thread_count} threads):"
    puts "    #{iterations} renders in #{fmt_ms(elapsed)}ms " \
         "| #{fmt_ops(iterations, elapsed)} ops/sec"
    puts
  end

  # ----- Mode 3: Multi-Ractor -----
  if %i[ractors all].include?(mode_filter)
    per_ractor = iterations / ractor_count
    extra = iterations % ractor_count

    t0 = Process.clock_gettime(Process::CLOCK_MONOTONIC)

    # Each Ractor creates its own Bundle instance — Ractors cannot share
    # mutable Ruby objects, but they can access shareable constants like
    # SSR::Deno::Bundle. The IsolatePool (Rust OnceLock) is process-shared,
    # so all Ractors dispatch through the same pool via round-robin.
    # The native extension declares rb_ext_ractor_safe(true) for safety.
    lib_path = File.join(BENCH_ROOT, 'lib')
    ractors = Array.new(ractor_count) do |i|
      count = per_ractor + (i < extra ? 1 : 0)
      Ractor.new(bundle_path, count, i, lib_path) do |path, iters, idx, lp|
        require 'json'
        require File.join(lp, 'ssr/deno')

        rb = SSR::Deno::Bundle.new(path)
        iters.times { rb.render({ data: { name: "R#{idx}" } }) }
      end
    end
    ractors.each(&:value)
    elapsed = Process.clock_gettime(Process::CLOCK_MONOTONIC) - t0

    puts "  Multi-Ractor (#{ractor_count} Ractors):"
    puts "    #{iterations} renders in #{fmt_ms(elapsed)}ms " \
         "| #{fmt_ops(iterations, elapsed)} ops/sec"
    puts
  end

  # ----- Mode 4: RactorPool (managed Ractors via RactorPool API) -----
  if %i[ractor_pool all].include?(mode_filter)
    timings = []

    t0 = Process.clock_gettime(Process::CLOCK_MONOTONIC)
    iterations.times do
      tc = Process.clock_gettime(Process::CLOCK_MONOTONIC)
      pool.render(payload)
      timings << (Process.clock_gettime(Process::CLOCK_MONOTONIC) - tc)
    end
    elapsed = Process.clock_gettime(Process::CLOCK_MONOTONIC) - t0

    sorted = timings.sort
    puts "  RactorPool (#{pool_size} workers):"
    puts "    #{iterations} renders in #{fmt_ms(elapsed)}ms " \
         "| #{fmt_ops(iterations, elapsed)} ops/sec " \
         "| p50: #{fmt_ms(percentile(sorted, 50))}ms " \
         "p99: #{fmt_ms(percentile(sorted, 99))}ms"
    puts
  end

  # ----- Memory Check -----
  final_heap = SSR::Deno.heap_stats['used_heap_size']
  delta = final_heap - initial_heap
  puts "  Memory: #{fmt_bytes(initial_heap)} → #{fmt_bytes(final_heap)} " \
       "(#{'+' if delta.positive?}#{fmt_bytes(delta.abs)})"
  puts
end

# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------

begin
  run_single_config(options)
rescue StandardError => error
  abort "Benchmark failed: #{error.message}\n#{error.backtrace.first(5).join("\n")}"
end
