# frozen_string_literal: true

require 'English'
require 'shellwords'

# All crate names (some depend on V8, can't test without full build).
CRATES = %w[
  ssr_deno_core
  ssr_deno_dev_mode
  ssr_deno_sys
].freeze

# Crates that compile without V8 (fast, no native build required).
V8_FREE_CRATES = %w[
  ssr_deno_core
  ssr_deno_sys
].freeze

V8_FREE_CRATES.each do |crate|
  desc "Run Rust unit tests for #{crate} (no V8 build required)"
  task "cargo:test:#{crate}" do
    sh 'cargo', 'test', '-p', crate, '--quiet', chdir: 'ext/ssr_deno'
  end
end

desc 'Run Rust unit tests for all V8-free crates'
task 'cargo:test' => V8_FREE_CRATES.map { |c| "cargo:test:#{c}" }

desc 'Run clippy lints on the ssr_deno crate'
task 'cargo:clippy' do
  sh 'cargo', 'clippy', '--', '-D', 'warnings', chdir: 'ext/ssr_deno'
end

desc 'Check Rust formatting'
task 'cargo:fmt' do
  sh 'cargo', 'fmt', '--check', chdir: 'ext/ssr_deno'
end

V8_FREE_CRATES.each do |crate|
  desc "Run Rust coverage for #{crate} (requires cargo-llvm-cov)"
  task "cargo:coverage:#{crate}" do
    subdir = 'ext/ssr_deno'
    cmd = %w[cargo llvm-cov --summary-only -p] << crate
    prefix = "cd #{subdir.shellescape} && "

    output = `#{prefix}#{cmd.shelljoin} 2>&1`
    abort 'cargo llvm-cov failed' unless $CHILD_STATUS.success?

    puts output

    pcts = output.scan(/\b(\d+\.\d+)%/).flatten
    line_pct = pcts.last&.to_f

    next unless line_pct&.positive?

    coverage_threshold = 90.0
    puts "#{crate} line coverage: #{line_pct}%"

    abort "#{crate} line coverage #{line_pct}% is below #{coverage_threshold}%" if line_pct < coverage_threshold
  end
end

desc 'Run Rust coverage for all V8-free crates (requires cargo-llvm-cov)'
task 'cargo:coverage' => V8_FREE_CRATES.map { |c| "cargo:coverage:#{c}" }
