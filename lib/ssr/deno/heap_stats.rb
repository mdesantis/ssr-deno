# frozen_string_literal: true

require 'json'

module SSR
  module Deno
    module HeapStats
      class << self
        # Returns V8 heap statistics from the isolate pool as a Hash.
        # Raises on error — use +fetch+ for a warning-only variant.
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
        def fetch!
          JSON.parse(SSR::Deno.native_heap_stats)
        rescue JSON::ParserError => error
          raise SSR::Deno::HeapStatsSerializationError, error.message
        end

        # @return [Hash<String, Integer>]
        def fetch
          fetch!
        rescue JsRuntimeNotInitializedError, JsRuntimeWorkerError,
               HeapStatsSerializationError => error
          warn "[ssr-deno] #{error.message}"
          {}
        end
      end
    end
  end
end
