# frozen_string_literal: true

require 'bundler/gem_tasks'
require 'dotenv/load'
require 'rake/extensiontask'
require 'minitest/test_task'
require 'rubocop/rake_task'

# Build-time environment variables (V8_FROM_SOURCE, GN_ARGS, LIBCLANG_PATH,
# RB_SYS_CARGO_PROFILE) are loaded from the .env file via dotenv. See
# .env.example for the documented defaults and plans/v8-tls-issue.md for the
# V8 build constraints.

Rake::ExtensionTask.new('ssr_deno') do |ext|
  ext.lib_dir = 'lib/ssr/deno'
end

# Default test suite — tests run without Node.js builtin support
# (node_builtins_enabled defaults to false).
Minitest::TestTask.create do |t|
  t.test_prelude = 'require "test/test_helper"'
end

# Override the default test file list to exclude the node_builtins suite.
# (We run it separately via test:node_builtins.)
Rake::Task[:test].clear if Rake::Task.task_defined?(:test)
namespace :test do
  root = __dir__
  lib = File.join(root, 'lib')
  test_dir = File.join(root, 'test')
  helper = File.join(test_dir, 'test_helper.rb')
  tmp = File.join(root, 'tmp')

  desc 'Run tests without Node.js builtin support'
  task :main do
    files = Dir.glob(File.join(test_dir, '**', 'test_*.rb'))
               .concat(Dir.glob(File.join(test_dir, '**', '*_test.rb')))
               .reject { |f| f.include?('test_integration_node_builtins') }
               .reject { |f| f.include?('test_helper') }
    runner = "require '#{helper}'\n"
    files.each { |f| runner << "require '#{f}'\n" }
    File.write(File.join(tmp, 'test_runner_main.rb'), runner)
    ruby "-I#{lib}:#{test_dir}", File.join(tmp, 'test_runner_main.rb')
  end
  desc 'Run tests that require Node.js builtin support'
  task :node_builtins do
    node_test = File.join(test_dir, 'ssr', 'test_integration_node_builtins.rb')
    runner = <<~RUBY
      require '#{helper}'
      SSR::Deno.node_builtins_enabled = true
      require '#{node_test}'
    RUBY
    File.write(File.join(tmp, 'test_runner_node.rb'), runner)
    ruby "-I#{lib}:#{test_dir}", File.join(tmp, 'test_runner_node.rb')
  end
end

# Alias `rake test` to run both suites
task test: %w[test:main test:node_builtins]

RuboCop::RakeTask.new

namespace :cargo do
  desc 'Run Rust unit tests for the ssr_deno_core crate (no V8 build required)'
  task :test do
    sh 'cargo', 'test', '-p', 'ssr_deno_core', chdir: 'ext/ssr_deno'
  end
end

SAMPLES = %w[
  react-ssr-app
  vanilla-ssr-app
  vue-ssr-app
  svelte-ssr-app
  react-mui-emotion-ssr-app
  react-mui-ssr-app
].freeze

namespace :samples do
  desc 'Build all SSR sample bundles'
  task build: SAMPLES.map { |s| "build:#{s}" }

  SAMPLES.each do |sample|
    desc "Build the #{sample} SSR bundle"
    task "build:#{sample}" do
      sh 'deno', 'task', 'build', chdir: "samples/#{sample}"
    end
  end
end

task default: %i[compile cargo:test samples:build test rubocop rbs]
