# frozen_string_literal: true

require 'minitest/test_task'

# Default Minitest task (overridden below to exclude node_builtins suite)
Minitest::TestTask.create

# Override test: clears the default Minitest task and splits into suites:
#   test:main         — default config
#   test:setters      — before pool init
#   test:node_builtins — node_builtins enabled, 2000ms timeout
#   test:async        — short 100ms timeout
#   test:env_config   — env var config
#   test:puma         — Puma integration (in-process single + clustered subprocess)
Rake::Task[:test].clear if Rake::Task.task_defined?(:test)

root = File.expand_path('..', __dir__)
lib = File.join(root, 'lib')
test_dir = File.join(root, 'test')
helper = File.join(test_dir, 'test_helper.rb')
tmp = File.join(root, 'tmp')

EXCLUDED_MAIN = %w[
  _node_builtins _async_render _setters
  _env_config _deno_rails _perf _puma
  test_helper
].freeze

desc 'Run tests without Node.js builtin support (default config)'
task 'test:main' do
  files = Dir.glob(File.join(test_dir, '**', 'test_*.rb'))
             .reject { |f| EXCLUDED_MAIN.any? { |p| f.include?(p) } }
  runner = <<~RUBY
    require '#{helper}'
    SSR::Deno.isolate_pool_size = 1
  RUBY
  files.each { |f| runner << "require '#{f}'\n" }
  File.write(File.join(tmp, 'test_runner_main.rb'), runner)
  ruby "-I#{lib}:#{test_dir}", File.join(tmp, 'test_runner_main.rb')
end

desc 'Run tests that require Node.js builtin support (node_builtins_enabled)'
task 'test:node_builtins' do
  node_test = File.join(test_dir, 'ssr', 'test_integration_node_builtins.rb')
  runner = <<~RUBY
    require '#{helper}'
    SSR::Deno.isolate_pool_size = 1
    SSR::Deno.render_timeout_ms = 2000
    SSR::Deno.node_builtins_enabled = true
    require '#{node_test}'
  RUBY

  File.write(File.join(tmp, 'test_runner_node.rb'), runner)
  sh({ 'SIMPLECOV_COMMAND_NAME' => 'test:node_builtins' },
     Gem.ruby, "-I#{lib}:#{test_dir}", File.join(tmp, 'test_runner_node.rb'))
end

desc 'Run setter API tests (must run before pool init)'
task 'test:setters' do
  setter_test = File.join(test_dir, 'ssr', 'test_deno_setters.rb')
  runner = <<~RUBY
    require '#{helper}'
    SSR::Deno.max_heap_size_mb = 128
    SSR::Deno.isolate_pool_size = 2
    SSR::Deno.render_timeout_ms = 500
    require '#{setter_test}'
  RUBY

  File.write(File.join(tmp, 'test_runner_setters.rb'), runner)
  sh({ 'SIMPLECOV_COMMAND_NAME' => 'test:setters' },
     Gem.ruby, "-I#{lib}:#{test_dir}", File.join(tmp, 'test_runner_setters.rb'))
end

desc 'Run async render tests with short timeout (render_timeout_ms=100)'
task 'test:async' do
  async_test = File.join(test_dir, 'ssr', 'test_deno_async_render.rb')
  runner = <<~RUBY
    require '#{helper}'
    SSR::Deno.isolate_pool_size = 1
    SSR::Deno.render_timeout_ms = 100
    require '#{async_test}'
  RUBY

  File.write(File.join(tmp, 'test_runner_async.rb'), runner)
  sh({ 'SIMPLECOV_COMMAND_NAME' => 'test:async' },
     Gem.ruby, "-I#{lib}:#{test_dir}", File.join(tmp, 'test_runner_async.rb'))
end

desc 'Run env var config tests'
task 'test:env_config' do
  env_config_test = File.join(test_dir, 'ssr', 'test_deno_env_config.rb')
  runner = <<~RUBY
    require '#{helper}'
    require '#{env_config_test}'
  RUBY

  File.write(File.join(tmp, 'test_runner_env_config.rb'), runner)
  sh({ 'SIMPLECOV_COMMAND_NAME' => 'test:env_config' },
     Gem.ruby, "-I#{lib}:#{test_dir}", File.join(tmp, 'test_runner_env_config.rb'))
end

desc 'Run Puma integration tests (in-process single mode + clustered subprocess)'
task 'test:puma' do
  puma_test = File.join(test_dir, 'ssr', 'test_integration_puma.rb')
  runner = <<~RUBY
    require '#{helper}'
    SSR::Deno.isolate_pool_size = 1
    SSR::Deno.render_timeout_ms = 5000
    require '#{puma_test}'
  RUBY

  File.write(File.join(tmp, 'test_runner_puma.rb'), runner)
  sh({ 'SIMPLECOV_COMMAND_NAME' => 'test:puma' },
     Gem.ruby, "-I#{lib}:#{test_dir}", File.join(tmp, 'test_runner_puma.rb'))
end

desc 'Run performance regression tests (pool=4, node_builtins)'
task 'test:perf' do
  perf_test = File.join(test_dir, 'ssr', 'test_perf.rb')
  runner = <<~RUBY
    ENV['SSR_DENO_SKIP_COVERAGE'] = 'true'
    require '#{helper}'
    ARGV.delete('--profile')
    SSR::Deno.isolate_pool_size = 4
    SSR::Deno.render_timeout_ms = 5000
    SSR::Deno.node_builtins_enabled = true
    require '#{perf_test}'
  RUBY

  File.write(File.join(tmp, 'test_runner_perf.rb'), runner)
  ruby "-I#{lib}:#{test_dir}", File.join(tmp, 'test_runner_perf.rb')
end

desc 'Run all test suites'
task test: %w[test:main test:setters test:node_builtins test:async test:env_config test:puma]

desc 'Check merged coverage (runs after test suites)'
task 'coverage:check' do
  require 'simplecov'
  require 'json'

  rs_path = File.join(SimpleCov.coverage_path, '.resultset.json')

  abort 'No coverage results — run `rake test` first' unless File.exist?(rs_path)

  results = SimpleCov::ResultMerger.merged_result
  stats = results.coverage_statistics

  line_stat = stats[:line]
  line_pct = line_stat&.percent

  # SimpleCov 0.22 doesn't surface branch stats in merged_result.
  # Compute from the raw resultset JSON instead.
  branch_pct = stats[:branch]&.percent

  unless branch_pct
    raw = JSON.parse(File.read(rs_path))
    merged_branches = {}

    raw.each_value do |suite_data|
      cov = suite_data['coverage']
      cov.each do |file_path, file_cov|
        next unless file_cov.is_a?(Hash) && file_cov['branches']

        merged_branches[file_path] ||= {}

        file_cov['branches'].each do |branch_key, conditions|
          merged_branches[file_path][branch_key] ||= {}

          conditions.each do |cond_key, count|
            existing = merged_branches[file_path][branch_key][cond_key] || 0
            merged_branches[file_path][branch_key][cond_key] = existing + count
          end
        end
      end
    end

    total = 0
    covered = 0

    merged_branches.each_value do |branches|
      branches.each_value do |conditions|
        conditions.each_value do |count|
          total += 1
          covered += 1 if count.positive?
        end
      end
    end

    branch_pct = total.positive? ? (covered.to_f / total * 100) : nil
  end

  puts "Merged line coverage: #{line_pct&.round(2)}%"
  puts "Merged branch coverage: #{branch_pct&.round(2)}%" if branch_pct

  abort "Merged line coverage #{line_pct.round(2)}% is below 100%" if line_pct && line_pct < 100.0
  abort "Merged branch coverage #{branch_pct.round(2)}% is below 100%" if branch_pct && branch_pct < 100.0
end
