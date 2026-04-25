# frozen_string_literal: true

require 'bundler/gem_tasks'
require 'minitest/test_task'
require 'rake/extensiontask'

Minitest::TestTask.create

require 'rubocop/rake_task'

RuboCop::RakeTask.new

Rake::ExtensionTask.new('ssr_deno') do |ext|
  ext.lib_dir = 'lib/ssr/deno'
  ext.source_pattern = '*.rs'
end

task default: %i[compile test rubocop]
