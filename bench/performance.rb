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
#   ruby bench/performance.rb                     # all pool sizes
#   ruby bench/performance.rb --pool-size 4       # single config
#   ruby bench/performance.rb --mode threads      # single mode
#   ruby bench/performance.rb --bundle react      # React SSR bundle
#   ruby bench/performance.rb --bundle mui-dashboard --node-builtins --timeout 30000
#
# Requires: compiled native extension (bundle exec rake compile)
# ---------------------------------------------------------------------------

require 'json'
require 'optparse'
require 'etc'
require 'fileutils'

# ---------------------------------------------------------------------------
# Paths
# ---------------------------------------------------------------------------

BENCH_ROOT = File.expand_path('..', __dir__).freeze
FIXTURES_DIR = File.join(BENCH_ROOT, 'test', 'fixtures').freeze
TMP_DIR = File.join(BENCH_ROOT, 'tmp').freeze
MINIMAL_BUNDLE = File.join(FIXTURES_DIR, 'minimal-bundle.js').freeze
REACT_BUNDLE = File.join(TMP_DIR, 'react-ssr-bundle.js').freeze
MUI_DASHBOARD_BUNDLE = File.join(TMP_DIR, 'react-mui-dashboard-ssr-bundle.js').freeze
SAMPLES_DIR = File.join(BENCH_ROOT, 'samples').freeze

# ---------------------------------------------------------------------------
# Config
# ---------------------------------------------------------------------------

BUNDLE_ALIASES = {
  'minimal' => MINIMAL_BUNDLE,
  'react' => REACT_BUNDLE,
  'mui-dashboard' => MUI_DASHBOARD_BUNDLE,
}.freeze

# Source samples for auto-build.
SAMPLE_SOURCES = {
  'react' => {
    dir: File.join(SAMPLES_DIR, 'vite-react-ssr-app'),
    build_out: 'dist/server/entry-server.js',
  },
  'mui-dashboard' => {
    dir: File.join(SAMPLES_DIR, 'vite-react-emotion-mui-dashboard-ssr-app'),
    build_out: 'dist/server/entry-server.js',
  },
}.freeze

options = {
  iterations: 1_000,
  warmup: 50,
  thread_count: 4,
  ractor_count: 4,
  pool_sizes: [1, 4, 0],
  mode: nil,
  subprocess: false,
  bundle: 'minimal',
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

  opts.on('-p', '--pool-size N', Integer, 'Run single pool size') do |n|
    options[:pool_sizes] = [n]
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

  opts.on('--bundle NAME', BUNDLE_ALIASES.keys.map(&:to_s),
          "Bundle: #{BUNDLE_ALIASES.keys.join(' / ')} (default: minimal)") do |b|
    options[:bundle] = b
  end

  opts.on('--subprocess', 'Internal: run as a subprocess') do
    options[:subprocess] = true
  end
end.parse!

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def ensure_bundle(bundle_alias, bundle_path)
  return if File.exist?(bundle_path)

  src = SAMPLE_SOURCES[bundle_alias]
  unless src
    raise "Bundle '#{bundle_alias}' not found at #{bundle_path}. " \
          "Build it first: cd #{File.join(SAMPLES_DIR, bundle_alias)} && deno task build"
  end

  sample_dir = src[:dir]
  build_out = File.join(sample_dir, src[:build_out])

  unless File.exist?(build_out)
    puts "  Building #{bundle_alias} bundle..."
    success = system('deno', 'task', 'build', chdir: sample_dir)
    abort "#{bundle_alias} build failed" unless success
    unless File.exist?(build_out)
      abort "Build succeeded but #{build_out} not found"
    end
  end

  FileUtils.cp(build_out, bundle_path)
  puts "  Copied #{bundle_alias} bundle to #{bundle_path}"
end

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
  pool_size = options[:pool_sizes].first
  iterations = options[:iterations]
  warmup = options[:warmup]
  thread_count = options[:thread_count]
  ractor_count = options[:ractor_count]
  mode_filter = options[:mode]
  bundle_path = BUNDLE_ALIASES[options[:bundle]] || options[:bundle]

  # Ensure bundle exists — build from sample if needed.
  ensure_bundle(options[:bundle], bundle_path)

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
  puts "  Bundle: #{bundle_label}"
  puts "  Node builtins: #{SSR::Deno.node_builtins_enabled?}"
  initial_heap = SSR::Deno.heap_stats['used_heap_size']
  configured_pool = SSR::Deno.isolate_pool_size
  # Match Rust's resolve_pool_size: 0 = auto-detect, else clamp to [1, 8].
  actual_pool = if configured_pool == 0
    (Etc.nprocessors - 1).clamp(1, 8)
  else
    configured_pool.clamp(1, 8)
  end

  puts
  puts "--- Pool Size: #{resolve_pool_label(pool_size)} (actual: #{actual_pool}) ---"
  puts "  Heap: #{fmt_bytes(initial_heap)}"
  puts

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
# Orchestrator — spawns subprocesses per pool size
# ---------------------------------------------------------------------------

def run_orchestrator(options)
  version_file = File.join(BENCH_ROOT, 'lib', 'ssr', 'deno', 'version.rb')
  version = begin
    content = File.read(version_file)
    content[/VERSION\s*=\s*['"]([^'"]+)['"]/, 1] || 'unknown'
  rescue
    'unknown'
  end

  puts "=" * 60
  puts "ssr-deno Performance Benchmark"
  nb = options[:node_builtins] ? ' node_builtins' : ''
  to = options[:timeout_ms] ? " timeout=#{options[:timeout_ms]}ms" : ''
  puts "Ruby: #{RUBY_VERSION} | ssr-deno: #{version} | bundle: #{options[:bundle]}#{nb}#{to}"
  puts "=" * 60

  script = File.expand_path(__FILE__)
  base_args = %W[
    --iterations #{options[:iterations]}
    --warmup #{options[:warmup]}
    --threads #{options[:thread_count]}
    --ractors #{options[:ractor_count]}
    --subprocess
  ]
  base_args << "--mode" << options[:mode].to_s if options[:mode]
  base_args << "--bundle" << options[:bundle]
  base_args << "--node-builtins" if options[:node_builtins]
  base_args << "--timeout" << options[:timeout_ms].to_s if options[:timeout_ms]

  options[:pool_sizes].each do |size|
    args = base_args + %w[--pool-size] + [size.to_s]
    puts
    puts "Running pool-size=#{resolve_pool_label(size)}..."
    success = system(RbConfig.ruby, script, *args)
    abort "Subprocess failed for pool-size=#{resolve_pool_label(size)}" unless success
  end

  puts "=" * 60
  puts "Benchmark complete"
end

# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------

begin
  if options[:subprocess]
    run_single_config(options)
  else
    run_orchestrator(options)
  end
rescue => e
  abort "Benchmark failed: #{e.message}\n#{e.backtrace.first(5).join("\n")}"
end
