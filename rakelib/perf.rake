# frozen_string_literal: true

root = File.expand_path('..', __dir__)
lib = File.join(root, 'lib')
test_dir = File.join(root, 'test')
helper = File.join(test_dir, 'test_helper.rb')
tmp = File.join(root, 'tmp')

desc 'Run ad-hoc benchmark for a sample (env: SAMPLE, POOL, MODE, ITERATIONS, WARMUP, TIMEOUT, NODE_BUILTINS)

Examples:
  rake perf:sample                            # minimal, pool=4
  rake perf:sample SAMPLE=react POOL=1        # React, single isolate
  rake perf:sample SAMPLE=mui-emotion NODE_BUILTINS=1 TIMEOUT=5000
  rake perf:sample SAMPLE=minimal MODE=ractors ITERATIONS=100'
task 'perf:sample' do
  sample = ENV.fetch('SAMPLE', 'minimal')
  pool = ENV.fetch('POOL', '4')
  mode = ENV.fetch('MODE', nil)
  iters = ENV.fetch('ITERATIONS', '500')
  warmup = ENV.fetch('WARMUP', '20')

  args = %W[--bundle #{sample} --pool-size #{pool} --iterations #{iters} --warmup #{warmup}]
  args += %W[--mode #{mode}] if mode
  args += %W[--node-builtins] if ENV['NODE_BUILTINS']
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
