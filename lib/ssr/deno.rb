# frozen_string_literal: true

require 'json'
require_relative 'deno/version'

# Load the native Rust extension (compiled by rb-sys / rake-compiler)
require_relative 'deno/ssr_deno'
require_relative 'deno/config'
require_relative 'deno/instrumenter'
require_relative 'deno/bundle'
require_relative 'deno/ractor_pool'

module SSR
  module Deno
    class << self
      # Returns V8 heap statistics from the isolate pool as a Hash.
      # Returns an empty Hash and prints a warning if the runtime is not yet
      # initialized (no Bundle.new has been called yet).
      #
      # Exposed counters (all Integer):
      #   total_heap_size             – total V8 heap usage (bytes)
      #   total_heap_size_executable  – executable memory (bytes)
      #   total_physical_size         – resident set size within V8 (bytes)
      #   total_available_size        – remaining heap before limit (bytes)
      #   used_heap_size              – live JS objects (bytes)
      #   heap_size_limit             – max heap size (configurable via max_heap_size_mb=)
      #   malloced_memory             – C++ memory allocated by V8 (bytes)
      #   external_memory             – memory held by V8 external references (bytes)
      #   peak_malloced_memory        – peak C++ allocation (bytes)
      #   number_of_native_contexts   – active V8 contexts
      #   number_of_detached_contexts – orphaned contexts
      #   total_global_handles_size   – persistent handle storage (bytes)
      #   used_global_handles_size    – live persistent handles (bytes)
      #
      # @return [Hash<String, Integer>]
      # @raise [JsRuntimeNotInitializedError] if pool not initialized
      # @raise [JsRuntimeWorkerError] if worker thread has exited
      # @raise [HeapStatsSerializationError] if native JSON is malformed
      def heap_stats!
        JSON.parse(native_heap_stats)
      rescue JSON::ParserError => error
        raise SSR::Deno::HeapStatsSerializationError, error.message
      end

      # @return [Hash<String, Integer>]
      def heap_stats
        heap_stats!
      rescue JsRuntimeNotInitializedError, JsRuntimeWorkerError,
             HeapStatsSerializationError => error
        warn "[ssr-deno] #{error.message}"
        {}
      end
    end
  end
end
