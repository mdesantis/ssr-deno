#!/usr/bin/env ruby
# frozen_string_literal: true

# PoC: ssr-deno gem runs without Rust/V8 build deps.
# Only the compiled .so + pure Ruby files needed at runtime.

$LOAD_PATH.unshift(File.join(__dir__, 'lib'))

require 'ssr/deno'

SSR::Deno.max_heap_size_mb = 64
SSR::Deno.render_timeout_ms = 10_000

begin
  bundle = SSR::Deno::Bundle.new(File.join(__dir__, 'minimal-bundle.js'))

  result = bundle.render({ data: { name: 'Docker PoC' } })

  puts "SSR result: #{result}"
  puts 'OK: gem works without Rust/V8 build deps at runtime.'
rescue => e
  warn "FAIL: #{e.class}: #{e.message}"
  warn e.backtrace.first(3).join("\n")
  exit 1
end
