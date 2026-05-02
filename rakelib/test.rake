# frozen_string_literal: true

require 'minitest/test_task'

# Default Minitest task (overridden below to exclude node_builtins suite)
Minitest::TestTask.create

# Override test: clears the default Minitest task and splits into two suites:
#   test:main         — 52 tests, node_builtins disabled (default)
#   test:node_builtins —  1 test,  node_builtins enabled
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
             .reject { |f| f.include?('test_helper') }
  runner = "require '#{helper}'\n"
  files.each { |f| runner << "require '#{f}'\n" }
  File.write(File.join(tmp, 'test_runner_main.rb'), runner)
  ruby "-I#{lib}:#{test_dir}", File.join(tmp, 'test_runner_main.rb')
end

desc 'Run tests that require Node.js builtin support (node_builtins_enabled)'
task 'test:node_builtins' do
  node_test = File.join(test_dir, 'ssr', 'test_integration_node_builtins.rb')
  runner = <<~RUBY
    require '#{helper}'
    SSR::Deno.node_builtins_enabled = true
    require '#{node_test}'
  RUBY
  File.write(File.join(tmp, 'test_runner_node.rb'), runner)
  ruby "-I#{lib}:#{test_dir}", File.join(tmp, 'test_runner_node.rb')
end

desc 'Run all test suites'
task test: %w[test:main test:node_builtins]
