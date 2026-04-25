# frozen_string_literal: true

require 'bundler/gem_tasks'
require 'rake/extensiontask'
require 'minitest/test_task'
require 'rubocop/rake_task'

Rake::ExtensionTask.new('ssr_deno') do |ext|
  ext.lib_dir = 'lib/ssr/deno'
  ext.source_pattern = '*.rs'
end

Minitest::TestTask.create

RuboCop::RakeTask.new

task default: %i[compile test rubocop]
