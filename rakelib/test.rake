# frozen_string_literal: true

require 'minitest/test_task'

# Default Minitest task (overridden below to exclude node_builtins suite)
Minitest::TestTask.create

# Override test: clears the default Minitest task and splits into suites:
#   test:main         — 50 tests (default config)
#   test:setters      —  4 tests (before pool init)
#   test:node_builtins —  2 tests (node_builtins enabled, 2000ms timeout)
#   test:async        —  6 tests (short 100ms timeout)
Rake::Task[:test].clear if Rake::Task.task_defined?(:test)

root = File.expand_path('..', __dir__)
lib = File.join(root, 'lib')
test_dir = File.join(root, 'test')
helper = File.join(test_dir, 'test_helper.rb')
tmp = File.join(root, 'tmp')

desc 'Run tests without Node.js builtin support (default config)'
task 'test:main' do
  files = Dir.glob(File.join(test_dir, '**', 'test_*.rb'))
             .concat(Dir.glob(File.join(test_dir, '**', '*_test.rb')))
             .reject { |f| f.include?('test_integration_node_builtins') }
             .reject { |f| f.include?('test_deno_async_render') }
             .reject { |f| f.include?('test_deno_setters') }
             .reject { |f| f.include?('test_helper') }
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

desc 'Run all test suites'
task test: %w[test:main test:setters test:node_builtins test:async]

desc 'Check merged coverage (runs after test:node_builtins)'
task 'coverage:check' do
  require 'simplecov'
  require 'json'

  rs_path = File.join(SimpleCov.coverage_path, '.resultset.json')

  abort 'No coverage results — run `rake test` first' unless File.exist?(rs_path)

  results = SimpleCov::ResultMerger.merged_result
  line = results.covered_percentages[:line]
  branch = results.covered_percentages[:branch]

  puts "Merged line coverage: #{line.round(2)}%"
  puts "Merged branch coverage: #{branch.round(2)}%" if branch

  abort "Merged line coverage #{line.round(2)}% is below 100%" if line < 100.0
  abort "Merged branch coverage #{branch.round(2)}% is below 100%" if branch && branch < 100.0
end
