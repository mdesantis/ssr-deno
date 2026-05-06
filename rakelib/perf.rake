# frozen_string_literal: true

root = File.expand_path('..', __dir__)
lib = File.join(root, 'lib')
samples_dir = File.join(root, 'samples')
test_dir = File.join(root, 'test')
helper = File.join(test_dir, 'test_helper.rb')
tmp = File.join(root, 'tmp')

desc 'Check performance regression (via test:perf)'
task 'perf:check' => 'test:perf'

desc 'Run ad-hoc benchmark against a sample directory (env: SAMPLE, POOL, MODE, ITERATIONS, WARMUP, TIMEOUT, NODE_BUILTINS)

Examples:
  rake perf:sample                       # samples/minimal? no — samples/vite-react-ssr-app
  rake perf:sample SAMPLE=vite-react-ssr-app POOL=1
  rake perf:sample SAMPLE=vite-react-mui-emotion-ssr-app NODE_BUILTINS=1 TIMEOUT=5000
  rake perf:sample SAMPLE=webpack-react-ssr-app MODE=single
  rake perf:sample SAMPLE=vite-vue-ssr-app POOL=4 ITERATIONS=200'
task 'perf:sample' do
  sample = ENV.fetch('SAMPLE', 'vite-react-ssr-app')
  pool = ENV.fetch('POOL', '4')
  mode = ENV.fetch('MODE', nil)
  iters = ENV.fetch('ITERATIONS', '200')
  warmup = ENV.fetch('WARMUP', '10')

  sample_dir = File.join(samples_dir, sample)
  bundle_path = File.join(sample_dir, 'dist/server/entry-server.js')

  unless File.exist?(sample_dir)
    abort "Sample directory not found: #{sample_dir}. Available: #{Dir.glob("#{samples_dir}/*/").map { |d| File.basename(d) }.sort.join(', ')}"
  end

  unless File.exist?(bundle_path)
    puts "  Building #{sample}..."
    sh 'deno', 'task', 'build', chdir: sample_dir
    abort "#{bundle_path} not found after build" unless File.exist?(bundle_path)
  end

  # Infer node_builtins: check if bundle requires Node.js built-in modules.
  node_builtins = ENV['NODE_BUILTINS']
  if node_builtins.nil?
    node_builtins = File.read(bundle_path).match?(/(__)?require\(["'](stream|buffer|events|async_hooks|util)["']\)/) ? '1' : '0'
  end

  args = %W[--bundle #{bundle_path} --pool-size #{pool} --iterations #{iters} --warmup #{warmup}]
  args += %W[--mode #{mode}] if mode
  args += %W[--node-builtins] if node_builtins == '1'
  args += %W[--timeout #{ENV['TIMEOUT']}] if ENV['TIMEOUT']

  ruby 'bench/performance.rb', *args
end

desc 'Update performance baselines in test/fixtures/perf-baselines.yml'
task 'perf:baseline:update' do
  fixtures = File.join(test_dir, 'fixtures', 'perf-baselines.yml')
  runner_path = File.join(tmp, 'baseline_update.rb')
  File.write(runner_path, <<~RUBY)
    require '#{helper}'
    Warning[:experimental] = false
    SSR::Deno.isolate_pool_size = 4
    SSR::Deno.render_timeout_ms = 5000
    SSR::Deno.node_builtins_enabled = true
    require 'support/perf_helpers'
    include PerfHelpers
    include TestFixturePaths

    results = {}
    [
      { bundle: 'minimal', path: TestFixturePaths::MINIMAL_BUNDLE, iters: 100, warmup: 20 },
      { bundle: 'react', path: REACT_BUNDLE, iters: 20, warmup: 10 },
      { bundle: 'mui_emotion', path: MUI_EMOTION_BUNDLE, iters: 10, warmup: 5 },
    ].each do |cfg|
      r = benchmark_single(cfg[:path], iterations: cfg[:iters], warmup: cfg[:warmup])
      results["\#{cfg[:bundle]}_single"] = r[:ops]
      puts "  \#{cfg[:bundle]}: \#{r[:ops]} ops/sec"
    end

    PerfHelpers.write_baselines(results, '#{fixtures}')
    puts "Baselines written to #{fixtures}"
  RUBY

  ruby "-I#{lib}:#{test_dir}", runner_path
end
