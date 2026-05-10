#!/usr/bin/env ruby
# frozen_string_literal: true

require 'json'
require 'optparse'
require 'net/http'
require 'etc'
require 'fileutils'
require 'tmpdir'
require 'socket'

BENCH_ROOT = File.expand_path('..', __dir__)
FIXTURES_DIR = File.join(BENCH_ROOT, 'test', 'fixtures')
MINIMAL_BUNDLE = File.join(FIXTURES_DIR, 'minimal-bundle.js')

options = {
  port: nil,
  socket: nil,
  threads: 5,
  workers: 0,
  isolate_pool_size: nil,
  ractor_pool: false,
  bundle_path: MINIMAL_BUNDLE,
  clients: 10,
  duration: 10,
  warmup: 2,
  compare: false,
  server_only: false,
  second_extra: []
}

# --compare = argv separator. Left = global (both sides).
# Right = overrides for second run only.
compare_idx = ARGV.index('--compare')
if compare_idx
  options[:compare] = true
  options[:second_extra] = ARGV[(compare_idx + 1)..] || []
  ARGV.replace(ARGV[0...compare_idx])
end

OptionParser.new do |opts|
  opts.banner = "Usage: #{$PROGRAM_NAME} [global flags] --compare [right-side flags]\n  " \
                '--[no-]ractor-pool before --compare affects bundle, after affects ractor_pool'

  opts.on('--port N', Integer, 'TCP port (default: unix socket)') { |v| options[:port] = v }
  opts.on('--socket PATH', 'Unix socket path (default: auto tmpdir)') { |v| options[:socket] = v }
  opts.on('--threads N', Integer, "Puma threads (default: #{options[:threads]})") { |v| options[:threads] = v }
  opts.on('--workers N', Integer, "Puma workers (default: #{options[:workers]})") { |v| options[:workers] = v }
  opts.on('--[no-]ractor-pool', 'Use RactorPool (default: no, uses Bundle directly)') do |v|
    options[:ractor_pool] = v
  end
  opts.on('--isolate-pool-size N', Integer, 'V8 isolate pool size (default: threads × (workers + 1))') do |v|
    options[:isolate_pool_size] = v
  end
  opts.on('--bundle PATH', 'Bundle path (default: minimal fixture)') { |v| options[:bundle_path] = v }
  opts.on('--clients N', Integer, "Concurrent clients (default: #{options[:clients]})") { |v| options[:clients] = v }
  opts.on('--duration N', Integer, "Benchmark seconds (default: #{options[:duration]})") { |v| options[:duration] = v }
  opts.on('--warmup N', Integer, "Warmup seconds (default: #{options[:warmup]})") { |v| options[:warmup] = v }
  opts.on('--server-only', 'Print server addr, wait for external tool (Ctrl-C to stop)') do |v|
    options[:server_only] = v
  end
  opts.on('--compare', 'Separator: flags left = global, flags right = second-run only') do |_v|
    options[:compare] ||= nil
  end
end.parse!

# ---------------------------------------------------------------------------
# Rack app factory
# ---------------------------------------------------------------------------

def build_app(bundle_path, ractor_pool)
  if ractor_pool
    pool_mutex = Mutex.new
    pool = nil

    lambda do |_env|
      pool_mutex.synchronize { pool ||= SSR::Deno::RactorPool.new(bundle_path:) }
      body = pool.render({ data: { name: 'bench' } })
      [200, { 'content-type' => 'text/html' }, [body]]
    end
  else
    lambda do |_env|
      bundle = SSR::Deno::Bundle.new(bundle_path)
      body = bundle.render({ data: { name: 'bench' } })
      [200, { 'content-type' => 'text/html' }, [body]]
    end
  end
end

# ---------------------------------------------------------------------------
# Puma server — returns addr hash
# ---------------------------------------------------------------------------

def start_puma(app, options)
  require 'puma'
  require 'puma/configuration'
  require 'puma/launcher'

  socket_path = options[:socket]
  port = options[:port]

  config = Puma::Configuration.new do |c|
    if socket_path
      c.bind "unix://#{socket_path}"
    else
      c.port port || 0, '0.0.0.0'
    end
    c.app app
    c.quiet
    c.log_requests false
    c.threads options[:threads], options[:threads]
    c.workers options[:workers]
  end

  launcher = Puma::Launcher.new(config)
  thr = Thread.new { launcher.run }

  if socket_path
    wait_for_socket(socket_path)
    addr = { socket: socket_path }
  else
    sleep 0.2 until (real_port = begin; launcher.connected_ports.first; rescue StandardError; nil; end)
    sleep 0.2
    addr = { port: real_port }
  end

  [launcher, thr, addr]
end

def wait_for_socket(path, timeout: 15)
  start = Process.clock_gettime(Process::CLOCK_MONOTONIC)
  loop do
    UNIXSocket.new(path).close
    return
  rescue Errno::ENOENT, Errno::ECONNREFUSED
    elapsed = Process.clock_gettime(Process::CLOCK_MONOTONIC) - start
    raise "Timeout waiting for socket #{path}" if elapsed > timeout

    sleep 0.1
  end
end

# ---------------------------------------------------------------------------
# HTTP client helpers (Unix socket + TCP)
# ---------------------------------------------------------------------------

def http_get(addr, path = '/render')
  if addr[:socket]
    sock = UNIXSocket.new(addr[:socket])
    sock.write("GET #{path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
    raw = sock.read
    sock.close
    raw.split("\r\n\r\n", 2).last
  else
    Net::HTTP.get('localhost', path, addr[:port])
  end
end

# ---------------------------------------------------------------------------
# Built-in benchmark
# ---------------------------------------------------------------------------

def warmup(addr, clients:, duration: 2)
  deadline = Process.clock_gettime(Process::CLOCK_MONOTONIC) + duration

  threads = Array.new(clients) do
    Thread.new do
      loop do
        break if Process.clock_gettime(Process::CLOCK_MONOTONIC) >= deadline

        http_get(addr)
      rescue StandardError
        nil
      end
    end
  end
  threads.each(&:join)
end

def bench_http(addr, clients:, duration:)
  deadline = Process.clock_gettime(Process::CLOCK_MONOTONIC) + duration
  mutex = Mutex.new
  counts = Array.new(clients, 0)
  errors = Array.new(clients, 0)
  timings = []

  threads = Array.new(clients) do |i|
    Thread.new do
      loop do
        remaining = deadline - Process.clock_gettime(Process::CLOCK_MONOTONIC)
        break if remaining <= 0

        tc = Process.clock_gettime(Process::CLOCK_MONOTONIC)
        http_get(addr)
        elapsed = Process.clock_gettime(Process::CLOCK_MONOTONIC) - tc
        mutex.synchronize do
          counts[i] += 1
          timings << elapsed
        end
      rescue StandardError
        mutex.synchronize { errors[i] += 1 }
      end
    end
  end
  threads.each(&:join)

  total = counts.sum
  ops = (total / duration).round
  sorted = timings.sort
  p50 = percentile(sorted, 50)
  p99 = percentile(sorted, 99)

  { ops:, p50_ms: (p50 * 1000).round(1), p99_ms: (p99 * 1000).round(1), errors: errors.sum, total: }
end

def percentile(sorted, pct)
  return 0.0 if sorted.empty?

  idx = [(pct.to_f / 100) * sorted.size, sorted.size - 1].min
  sorted[idx.to_i]
end

# ---------------------------------------------------------------------------
# Vegeta benchmark
# ---------------------------------------------------------------------------

def detect_vegeta
  system('which vegeta', out: '/dev/null') ? 'vegeta' : nil
end

def bench_vegeta(addr, clients:, duration:)
  vegeta = detect_vegeta
  return nil unless vegeta

  results_file = File.join(Dir.tmpdir, "vegeta-#{Process.pid}.bin")
  targets_file = File.join(Dir.tmpdir, "vegeta-targets-#{Process.pid}.txt")
  target = addr[:port] ? "GET http://localhost:#{addr[:port]}/render\n" : "GET http://localhost/render\n"
  File.write(targets_file, target)

  args = [vegeta, 'attack', '-rate=0', "-duration=#{duration}s",
          "-workers=#{clients}", "-max-workers=#{clients * 2}",
          "-targets=#{targets_file}", "-output=#{results_file}"]
  args.push('-unix-socket', addr[:socket]) if addr[:socket]

  system(*args, out: '/dev/null', err: '/dev/null')
  FileUtils.rm_f(targets_file)
  report = `#{vegeta} report #{results_file} 2>&1`
  FileUtils.rm_f(results_file)
  report
end

# ---------------------------------------------------------------------------
# Run single benchmark
# ---------------------------------------------------------------------------

def run_bench(addr, options)
  puts
  puts 'Warming up...'
  warmup(addr, clients: options[:clients], duration: options[:warmup])

  puts "Benchmarking (#{options[:duration]}s, #{options[:clients]} clients)..."
  vegeta_output = bench_vegeta(addr, clients: options[:clients], duration: options[:duration])

  if vegeta_output
    puts vegeta_output
    { raw: vegeta_output }
  else
    puts '  vegeta not found — falling back to built-in HTTP client'
    result = bench_http(addr, clients: options[:clients], duration: options[:duration])
    result[:duration] = options[:duration]
    print_result(options[:ractor_pool] ? 'RactorPool' : 'Bundle', result)
    result
  end
end

def print_result(label, result)
  puts "  #{label}:"
  puts "    #{result[:total]} requests in #{result[:duration]}s" if result[:duration]
  puts "    #{result[:ops]} req/sec | p50: #{result[:p50_ms]}ms p99: #{result[:p99_ms]}ms | #{result[:errors]} errors"
end

def addr_label(addr)
  addr[:socket] ? "unix:#{addr[:socket]}" : "http://localhost:#{addr[:port]}/render"
end

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

begin
  # Resolve default transport
  if options[:port]
    # explicit TCP
  elsif options[:socket]
    FileUtils.mkdir_p(File.dirname(options[:socket]))
  else
    tmpdir = Dir.mktmpdir('ssr-deno-throughput')
    options[:socket] = File.join(tmpdir, 'ssr.sock')
  end

  if options[:compare]
    global = ['--clients', options[:clients].to_s,
              '--duration', options[:duration].to_s,
              '--warmup', options[:warmup].to_s,
              '--bundle', options[:bundle_path]]
    if options[:socket]
      global.push('--socket', options[:socket])
    elsif options[:port]
      global.push('--port', options[:port].to_s)
    end

    left_argv = [*global, '--no-ractor-pool']
    left_argv.push('--workers', options[:workers].to_s) if options[:workers].positive?
    left_argv.push('--threads', options[:threads].to_s) if options[:threads] != 5
    left_argv.push('--isolate-pool-size', options[:isolate_pool_size].to_s) if options[:isolate_pool_size]

    right_argv = [*global, '--no-ractor-pool', *options[:second_extra]]

    results = {}
    { 'left' => left_argv, 'right' => right_argv }.each do |key, argv|
      label = argv.include?('--ractor-pool') ? 'RactorPool' : 'Bundle'
      puts
      puts "=== #{key}: #{label} ==="
      dir = Dir.mktmpdir('ssr-deno-throughput')
      args = [Gem.ruby, $PROGRAM_NAME, *argv]

      pid = spawn({ 'BUNDLE_GEMFILE' => File.join(BENCH_ROOT, 'Gemfile'), 'PATH' => ENV.fetch('PATH', nil) },
                  *args, chdir: BENCH_ROOT,
                         out: File.join(dir, 'out.log'),
                         err: File.join(dir, 'err.log'))
      Process.wait(pid)
      output = File.read(File.join(dir, 'out.log'))
      err_output = File.read(File.join(dir, 'err.log'))
      FileUtils.rm_rf(dir)

      results[key] = output
      puts output
      puts "  [stderr] #{err_output.lines.first&.strip}" unless err_output.strip.empty?
    end

    puts
    puts '=' * 60
    puts 'COMPARISON'
    puts '=' * 60
    [['left', left_argv], ['right', right_argv]].each do |key, argv|
      label = argv.include?('--ractor-pool') ? 'RactorPool' : 'Bundle'
      output = results[key]
      lines = output.lines.select { |l| l.match?(/Requests\s+\[/) || l.match?(/Latencies\s+\[/) }
      puts
      puts "  #{label} (#{key}):"
      lines.each { |l| puts "    #{l.strip}" }
    end
  else
    $LOAD_PATH.unshift File.join(BENCH_ROOT, 'lib')
    Warning[:experimental] = false if Warning.respond_to?(:[])
    require 'ssr/deno'
    SSR::Deno.isolate_pool_size = options[:isolate_pool_size] || (options[:threads] * (options[:workers] + 1))
    SSR::Deno.render_timeout_ms = 2000

    app = build_app(options[:bundle_path], options[:ractor_pool])
    launcher, thr, addr = start_puma(app, options)
    label = options[:ractor_pool] ? 'RactorPool' : 'Bundle'
    puts "Server at #{addr_label(addr)} (#{label})"

    if options[:server_only]
      puts 'Ctrl-C to stop'
      sleep
    else
      run_bench(addr, options)
    end
  end
rescue Interrupt
  puts
rescue StandardError => error
  abort "Error: #{error.message}\n#{error.backtrace&.first(3)&.join("\n")}"
ensure
  if local_variables.include?(:launcher) && launcher
    launcher.stop
    thr&.join(3)
  end
  FileUtils.rm_f(options[:socket]) if options[:socket] && File.exist?(options[:socket])
end
