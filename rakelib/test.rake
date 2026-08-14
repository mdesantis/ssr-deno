# frozen_string_literal: true

require 'minitest/test_task'

# Default Minitest task (cleared and replaced below by the per-suite split;
# see EXCLUDED_MAIN for what test:main leaves out)
Minitest::TestTask.create

# Override test: clears the default Minitest task and splits into suites:
#   test:main          — default config
#   test:config        — before pool init
#   test:node_builtins — node_builtins enabled, 2000ms timeout
#   test:async         — short 100ms timeout
#   test:env_config    — env var config
#   test:ractor        — RactorPool, pool_size=4, 5000ms timeout
#   test:puma          — Puma integration (in-process single + clustered subprocess)
#   test:rails         — Railtie/helper/LogSubscriber via Combustion
#   test:perf          — performance regression, runs standalone via perf:check
Rake::Task[:test].clear if Rake::Task.task_defined?(:test)

root = File.expand_path('..', __dir__)
lib = File.join(root, 'lib')
test_dir = File.join(root, 'test')
helper = File.join(test_dir, 'test_helper.rb')
tmp = File.join(root, 'tmp')

EXCLUDED_MAIN = %w[
  _node_builtins _async_render _config
  _env_config _deno_rails _perf _puma
  _ractor_pool test_helper
].freeze

desc 'Run tests without Node.js builtin support (default config)'
task 'test:main' do
  files = Dir.glob(File.join(test_dir, '**', 'test_*.rb'))
             .reject { |f| EXCLUDED_MAIN.any? { |p| f.include?(p) } }
  runner = <<~RUBY
    require '#{helper}'
    SSR::Deno::Config.isolate_pool_size = 1
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
    SSR::Deno::Config.isolate_pool_size = 1
    SSR::Deno::Config.render_timeout_ms = 2000
    SSR::Deno::Config.node_builtins_enabled = true
    require '#{node_test}'
  RUBY

  File.write(File.join(tmp, 'test_runner_node.rb'), runner)
  sh({ 'SIMPLECOV_COMMAND_NAME' => 'test:node_builtins' },
     Gem.ruby, "-I#{lib}:#{test_dir}", File.join(tmp, 'test_runner_node.rb'))
end

desc 'Run config API tests (must run before pool init)'
task 'test:config' do
  config_test = File.join(test_dir, 'ssr', 'test_deno_config.rb')
  runner = <<~RUBY
    require '#{helper}'
    SSR::Deno::Config.max_heap_size_mb = 128
    SSR::Deno::Config.isolate_pool_size = 2
    SSR::Deno::Config.render_timeout_ms = 500
    require '#{config_test}'
  RUBY

  File.write(File.join(tmp, 'test_runner_config.rb'), runner)
  sh({ 'SIMPLECOV_COMMAND_NAME' => 'test:config' },
     Gem.ruby, "-I#{lib}:#{test_dir}", File.join(tmp, 'test_runner_config.rb'))
end

desc 'Run async render tests with short timeout (render_timeout_ms=100)'
task 'test:async' do
  async_test = File.join(test_dir, 'ssr', 'test_deno_async_render.rb')
  runner = <<~RUBY
    require '#{helper}'
    SSR::Deno::Config.isolate_pool_size = 1
    SSR::Deno::Config.render_timeout_ms = 100
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

desc 'Run RactorPool tests (isolate_pool_size=4, long timeout)'
task 'test:ractor' do
  ractor_test = File.join(test_dir, 'ssr', 'test_ractor_pool.rb')
  runner = <<~RUBY
    require '#{helper}'
    Warning[:experimental] = false if Warning.respond_to?(:[])
    SSR::Deno::Config.isolate_pool_size = 4
    SSR::Deno::Config.render_timeout_ms = 5000
    require '#{ractor_test}'
  RUBY

  File.write(File.join(tmp, 'test_runner_ractor.rb'), runner)
  sh({ 'SIMPLECOV_COMMAND_NAME' => 'test:ractor' },
     Gem.ruby, "-I#{lib}:#{test_dir}", File.join(tmp, 'test_runner_ractor.rb'))
end

desc 'Run Puma integration tests (in-process single mode + clustered subprocess)'
task 'test:puma' do
  puma_test = File.join(test_dir, 'ssr', 'test_integration_puma.rb')
  runner = <<~RUBY
    require '#{helper}'
    SSR::Deno::Config.isolate_pool_size = 1
    SSR::Deno::Config.render_timeout_ms = 5000
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
    SSR::Deno::Config.isolate_pool_size = 4
    SSR::Deno::Config.render_timeout_ms = 5000
    SSR::Deno::Config.node_builtins_enabled = true
    require '#{perf_test}'
  RUBY

  File.write(File.join(tmp, 'test_runner_perf.rb'), runner)
  ruby "-I#{lib}:#{test_dir}", File.join(tmp, 'test_runner_perf.rb')
end

desc 'Run Rails integration tests (Railtie, Helper via Combustion)'
task 'test:rails' do
  rails_test = File.join(test_dir, 'ssr', 'test_integration_deno_rails.rb')
  runner = <<~RUBY
    require '#{File.join(test_dir, 'test_helper_rails.rb')}'
    require '#{rails_test}'
  RUBY

  File.write(File.join(tmp, 'test_runner_rails.rb'), runner)
  sh({ 'SIMPLECOV_COMMAND_NAME' => 'test:rails' },
     Gem.ruby, "-I#{lib}:#{test_dir}", File.join(tmp, 'test_runner_rails.rb'))
end

desc 'Run all test suites'
task :test do
  ENV['SSR_DENO_SUPPRESS_COVERAGE_REPORT'] = 'true'
  %w[test:main test:config test:node_builtins test:async test:env_config test:ractor test:puma test:rails].each do |t|
    Rake::Task[t].invoke
  end
end

desc 'Check merged coverage (runs after test suites)'
task 'coverage:check' do
  require 'simplecov'

  line_threshold = ENV.fetch('SSR_DENO_COVERAGE_LINE_THRESHOLD', '100').to_f
  branch_threshold = ENV.fetch('SSR_DENO_COVERAGE_BRANCH_THRESHOLD', '100').to_f

  rs_path = File.join(SimpleCov.coverage_path, '.resultset.json')

  abort 'No coverage results — run `rake test` first' unless File.exist?(rs_path)

  SimpleCov.enable_coverage :branch

  results = SimpleCov::ResultMerger.merged_result

  unless results
    age = (Time.now - File.mtime(rs_path)).round

    abort "Coverage resultset is stale (#{age}s old, merge_timeout is " \
          "#{SimpleCov.merge_timeout}s) — run `rake test` again"
  end

  stats = results.coverage_statistics

  line_pct = stats[:line]&.percent
  branch_pct = stats[:branch]&.percent

  abort 'Branch coverage was not measured (coverage_statistics[:branch] is nil)' unless branch_pct

  puts "Merged line coverage: #{line_pct&.round(2)}%"
  puts "Merged branch coverage: #{branch_pct.round(2)}%"

  results.format!

  if line_pct && line_pct < line_threshold
    results.original_result.each do |file_path, file_cov|
      lines = file_cov.is_a?(Hash) ? file_cov['lines'] : file_cov
      next unless lines

      lines.each_with_index do |count, idx|
        puts "  UNCOVERED: #{file_path}:#{idx + 1}" if count.is_a?(Integer) && count.zero?
      end
    end
    abort "Merged line coverage #{line_pct.round(2)}% is below #{line_threshold}%"
  end
  abort "Merged branch coverage #{branch_pct.round(2)}% is below #{branch_threshold}%" if branch_pct < branch_threshold
end
