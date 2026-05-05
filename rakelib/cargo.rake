# frozen_string_literal: true

desc 'Run Rust unit tests for the ssr_deno_core crate (no V8 build required)'
task 'cargo:test' do
  sh 'cargo', 'test', '-p', 'ssr_deno_core', chdir: 'ext/ssr_deno'
end

desc 'Run clippy lints on the ssr_deno crate'
task 'cargo:clippy' do
  sh 'cargo', 'clippy', '--', '-D', 'warnings', chdir: 'ext/ssr_deno'
end
