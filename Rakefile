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

# Task files in rakelib/:
#   cargo.rake   — cargo:test, cargo:clippy, cargo:fmt, cargo:coverage
#   perf.rake    — perf:check, perf:baseline:update
#   samples.rake — samples:build, samples:build:<name>
#   test.rake    — test:main, test:config, test:node_builtins, test:async, test:env_config, test:puma

RuboCop::RakeTask.new

RuboCop::RakeTask.new('rubocop:rails') do |task|
  task.patterns = ['lib/ssr/deno/rails/', 'lib/ssr/deno/rails.rb']
  task.options = ['--config', '.rubocop-rails.yml']
end

task default: %i[compile cargo:clippy cargo:test cargo:test:ssr_deno_dev_mode cargo:coverage cargo:fmt] +
              %i[samples:build test coverage:check perf:check rubocop rubocop:rails rbs]
