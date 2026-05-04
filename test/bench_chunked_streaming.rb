#!/usr/bin/env ruby
# frozen_string_literal: true

# Benchmark: poll-based vs. op-based chunked streaming.
# Measures wall-clock time for varying chunk counts.
#
# Usage: bundle exec ruby test/bench_chunked_streaming.rb

require_relative '../lib/ssr/deno'

POLL_BUNDLE = File.expand_path('fixtures/bench-chunked-poll.js', __dir__)
OP_BUNDLE = File.expand_path('fixtures/bench-chunked-op.js', __dir__)

poll = SSR::Deno::Bundle.new(POLL_BUNDLE)
op = SSR::Deno::Bundle.new(OP_BUNDLE)

CHUNK_COUNTS = [5, 10, 25, 50, 100, 250, 500, 1000].freeze
WARMUP = 3
ITERATIONS = 20

def measure(bundle, method, count)
  chunks = []

  t0 = Process.clock_gettime(Process::CLOCK_MONOTONIC)
  bundle.send(method, { count: count }) { |chunk| chunks << chunk }
  t1 = Process.clock_gettime(Process::CLOCK_MONOTONIC)

  elapsed_ms = (t1 - t0) * 1000.0

  raise "Expected #{count} chunks, got #{chunks.size}" unless chunks.size == count

  elapsed_ms
end

puts "=" * 70
puts "Chunked Streaming Benchmark: Poll-Based vs Op-Based"
puts "=" * 70
puts
puts format("%-8s | %-20s | %-20s | %s", "Chunks", "Poll (ms)", "Op (ms)", "Ratio")
puts "-" * 70

CHUNK_COUNTS.each do |count|
  # Warmup
  WARMUP.times do
    measure(poll, :render_stream_chunks, count)
    measure(op, :render_stream_chunks_op, count)
  end

  # Measure
  poll_times = ITERATIONS.times.map { measure(poll, :render_stream_chunks, count) }
  op_times = ITERATIONS.times.map { measure(op, :render_stream_chunks_op, count) }

  poll_median = poll_times.sort[ITERATIONS / 2]
  op_median = op_times.sort[ITERATIONS / 2]

  ratio = op_median / poll_median

  puts format(
    "%-8d | %6.2f (p50) %6.2f (p95) | %6.2f (p50) %6.2f (p95) | %.2fx",
    count,
    poll_median, poll_times.sort[(ITERATIONS * 0.95).to_i],
    op_median, op_times.sort[(ITERATIONS * 0.95).to_i],
    ratio
  )
end

puts
puts "Ratio > 1.0 means op-based is slower than poll-based."
puts "Ratio < 1.0 means op-based is faster than poll-based."
