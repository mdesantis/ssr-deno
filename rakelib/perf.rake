# frozen_string_literal: true

root = File.expand_path('..', __dir__)
lib = File.join(root, 'lib')
test_dir = File.join(root, 'test')
helper = File.join(test_dir, 'test_helper.rb')
tmp = File.join(root, 'tmp')

desc 'Check performance regression (via test:perf)'
task 'perf:check' => 'test:perf'

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
      rp = benchmark_ractor_pool(cfg[:path], iterations: cfg[:iters], warmup: cfg[:warmup], size: 4)
      results["\#{cfg[:bundle]}_ractor_pool"] = rp[:ops]
      puts "  \#{cfg[:bundle]} single: \#{r[:ops]} ops/sec | ractor_pool: \#{rp[:ops]} ops/sec"
    end

    PerfHelpers.write_baselines(results, '#{fixtures}')
    puts "Baselines written to #{fixtures}"
  RUBY

  ruby "-I#{lib}:#{test_dir}", runner_path
end
