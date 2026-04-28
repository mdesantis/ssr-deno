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

task default: %i[compile test rubocop]
