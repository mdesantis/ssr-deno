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

# Task definitions live in rakelib/ — cargo.rake, perf.rake, rbs.rake,
# samples.rake, test.rake. Run `rake -T` for the current list; it is not
# duplicated here because it goes stale.

RuboCop::RakeTask.new

RuboCop::RakeTask.new('rubocop:rails') do |task|
  task.patterns = ['lib/ssr/deno/rails/', 'lib/ssr/deno/rails.rb']
  task.options = ['--config', '.rubocop-rails.yml']
end

task default: %i[compile cargo:clippy cargo:test cargo:test:ssr_deno_dev_mode cargo:test:ssr_deno] +
              %i[cargo:coverage cargo:fmt samples:build test coverage:check perf:check] +
              %i[rubocop rubocop:rails rbs]
