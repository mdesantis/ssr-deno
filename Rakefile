# frozen_string_literal: true

require 'bundler/gem_tasks'
require 'rake/extensiontask'
require 'minitest/test_task'
require 'rubocop/rake_task'

# ---------------------------------------------------------------------------
# Guard: the `compile` task MUST be run via `./bin/compile`, which sets the
# V8_FROM_SOURCE and GN_ARGS environment variables required for a correct
# V8 build (see plans/v8-tls-issue.md).
#
# We hook into the platform-specific compile task by prepending a guard
# prerequisite. This catches all entry points (direct `rake compile`,
# `rake compile:ssr_deno`, etc.) because the platform task is always invoked.
# ---------------------------------------------------------------------------
desc 'Guard: ensure compile is run via ./bin/compile'
task :guard_compile_env do
  next if ENV['SSR_DENO_DEV_BIN_COMPILE'] == 'true'

  warn <<~MSG
    ERROR: The `compile` task must be run through `./bin/compile`.

      $ ./bin/compile

    This script sets the environment variables required to build V8 as a
    shared library (V8_FROM_SOURCE, GN_ARGS, LIBCLANG_PATH).

    See: plans/v8-tls-issue.md
  MSG
  exit 1
end

Rake::ExtensionTask.new('ssr_deno') do |ext|
  ext.lib_dir = 'lib/ssr/deno'
  ext.source_pattern = '*.rs'
  ext.extra_sources = FileList['ext/ssr_deno/src/*.rs']
end

# Prepend the guard as a prerequisite of the platform-specific compile task.
# This is the innermost task that actually invokes cargo/gmake.
platform_task = Rake::Task['compile:ssr_deno:x86_64-linux']
old_prereqs = platform_task.prerequisites.dup
platform_task.clear_prerequisites
platform_task.enhance([:guard_compile_env] + old_prereqs)

Minitest::TestTask.create

RuboCop::RakeTask.new

task default: %i[compile test rubocop]
