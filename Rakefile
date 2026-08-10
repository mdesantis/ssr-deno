# frozen_string_literal: true

require 'bundler/gem_tasks'
require 'dotenv/load'
require 'rake/extensiontask'
require 'rubocop/rake_task'

# Build-time environment variables (RB_SYS_CARGO_PROFILE, RUSTFLAGS, etc.)
# are loaded from the .env file via dotenv. See .env.example for documented options.

Rake::ExtensionTask.new('ssr_deno') do |ext|
  ext.lib_dir = 'lib/ssr/deno'
end

# Task files in rakelib/ (see each file for its exact task list — not
# duplicated here, it goes stale):
#   cargo.rake   — cargo:test(:<crate>), cargo:clippy, cargo:fmt, cargo:coverage
#   perf.rake    — perf:check, perf:baseline:update
#   rbs.rake     — rbs:validate, rbs:up_to_date, rbs:diff, rbs
#   samples.rake — samples:build(:<name>), samples:clean
#   test.rake    — test:main and 8 other suites, plus coverage:check

RuboCop::RakeTask.new

RuboCop::RakeTask.new('rubocop:rails') do |task|
  task.patterns = ['lib/ssr/deno/rails/', 'lib/ssr/deno/rails.rb']
  task.options = ['--config', '.rubocop-rails.yml']
end

task default: %i[compile cargo:clippy cargo:test cargo:test:ssr_deno_dev_mode cargo:test:ssr_deno] +
              %i[cargo:coverage cargo:fmt samples:build test coverage:check perf:check] +
              %i[rubocop rubocop:rails rbs]
