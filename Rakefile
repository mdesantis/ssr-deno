# frozen_string_literal: true

require 'bundler/gem_tasks'
require 'dotenv/load'
require 'rake/extensiontask'
require 'minitest/test_task'
require 'rubocop/rake_task'

# The V8 build environment variables (V8_FROM_SOURCE, GN_ARGS, LIBCLANG_PATH)
# are loaded from the .env file via dotenv (see .env.example).
# These are required to build V8 as a shared library (see plans/v8-tls-issue.md).

Rake::ExtensionTask.new('ssr_deno') do |ext|
  ext.lib_dir = 'lib/ssr/deno'
  ext.source_pattern = '*.rs'
  ext.extra_sources = FileList['ext/ssr_deno/src/*.rs']
end

Minitest::TestTask.create

RuboCop::RakeTask.new

task default: %i[compile test rubocop]
