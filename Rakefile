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

Minitest::TestTask.create do |t|
  t.test_prelude = 'require "test/test_helper"'
end

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
