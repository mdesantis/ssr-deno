#!/usr/bin/env ruby
# frozen_string_literal: true

# Reproduce: what happens when a user SSR component leaks memory past
# max_heap_size_mb on a V8 isolate.
#
# Run from repo root:  bundle exec ruby plans/attachments/reproduce_v8_oom.rb

$LOAD_PATH.unshift File.expand_path('../../lib', __dir__)
$stdout.sync = true

require 'ssr/deno'
require 'tmpdir'

MAX_HEAP_MB  = 16
LEAK_KB_PER_RENDER = 500   # allocate 500 KB per render
MAX_ITERATIONS = 500

LEAK_BUNDLE_JS = <<~JS
  var leak = [];
  globalThis.render = function() {
    // Allocate a large array that V8 cannot easily GC
    leak.push(new Array(#{LEAK_KB_PER_RENDER * 128}).fill('x'));
    return '<p>rendered ' + leak.length + '</p>';
  };
JS

puts "=== V8 OOM Reproduction ==="
puts "max_heap_size_mb: #{MAX_HEAP_MB} MB"
puts "leak per render:  #{LEAK_KB_PER_RENDER} KB"
puts

SSR::Deno.max_heap_size_mb = MAX_HEAP_MB
SSR::Deno.isolate_pool_size = 1

Dir.mktmpdir('ssr-deno-oom') do |dir|
  bundle_path = File.join(dir, 'leak-bundle.js')
  File.write(bundle_path, LEAK_BUNDLE_JS)

  bundle = SSR::Deno::Bundle.new(bundle_path)

  puts 'Iteration | used_heap_size | total_heap_size | limit | result'
  puts '----------|---------------|-----------------|-------|-------'

  MAX_ITERATIONS.times do |i|
    stats = SSR::Deno.heap_stats
    used_mb = stats['used_heap_size'] / (1024.0 * 1024.0)
    total_mb = stats['total_heap_size'] / (1024.0 * 1024.0)
    limit_mb = stats['heap_size_limit'] / (1024.0 * 1024.0)

    result = bundle.render({})
    puts format('%9d | %13.1f | %15.1f | %5.1f | %s', i + 1, used_mb, total_mb, limit_mb, result)

    sleep 0.01
  end
end

puts
puts 'Done — V8 did not crash.'
