# frozen_string_literal: true

require 'json'
require_relative 'deno/version'

# Load the native Rust extension (compiled by rb-sys / rake-compiler)
require_relative 'deno/ssr_deno'
require_relative 'deno/instrumenter'
require_relative 'deno/bundle'

module SSR
  module Deno
    class << self
      # Set the maximum V8 heap size in megabytes before initializing the runtime.
      # Must be called before any Bundle.new call (triggers pool init).
      #
      # max_heap_size_mb is a per-isolate V8 CreateParams constraint, NOT a
      # total process budget. Each isolate independently gets this limit,
      # regardless of pool size. This ensures workloads calibrated for a
      # single isolate don't break when the pool auto-detects more cores.
      #
      # Default: 64 MB — sensible for typical SSR workloads (~20 MB baseline +
      # bundle + render peak + headroom). Set to 0 for unlimited (V8 built-in
      # default, ~1.4 GB on 64-bit).
      #
      # @param mega_bytes [Integer] heap size in MB
      def max_heap_size_mb=(mega_bytes)
        native_set_max_heap_size_mb(mega_bytes.to_i)
      end

      # Set the number of V8 isolates in the pool before initializing the runtime.
      # Must be called before any Bundle.new call (triggers pool init).
      #
      # Default: 0 = auto-detect from CPU count (capped at 8, minus one for
      # the Ruby thread). A pool of 2–4 is typical for concurrent SSR.
      #
      # @param size [Integer] isolate count (0 = auto-detect, min 1, max 8)
      def isolate_pool_size=(size)
        native_set_isolate_pool_size(size.to_i)
      end

      # Set the render timeout in milliseconds before initializing the runtime.
      # Must be called before any Bundle.new call (triggers pool init).
      #
      # Default: 500ms — sensible for SSR where CSR fallback is the alternative.
      # Use shorter values (100ms+) for test environments; longer values
      # (up to 5min) for complex production pages.
      #
      # @param milliseconds [Integer] render timeout in ms (min 100, max 300000)
      # @raise [ArgumentError] if ms is out of valid range
      # @raise [JsRuntimeInitializationError] if pool already initialized
      def render_timeout_ms=(milliseconds)
        native_set_render_timeout_ms(milliseconds.to_i)
      end

      # Enable Node.js built-in module support (stream, buffer, events, etc.).
      # Required for packages like @emotion/server that call require() for
      # Node.js built-in modules. Default: false.
      #
      # When enabled, the Rust extension initializes a custom module loader
      # and injects a globalThis.require function via createRequire from
      # node:module. This adds ~50ms to worker initialization time.
      #
      # Must be called before any Bundle.new call (triggers pool init).
      #
      # @param enabled [Boolean]
      def node_builtins_enabled=(enabled)
        native_set_node_builtins_enabled(enabled)
      end

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
      def heap_stats!
        JSON.parse(native_heap_stats)
      end

      # @return [Hash<String, Integer>]
      def heap_stats
        heap_stats!
      rescue JsRuntimeNotInitializedError, JsRuntimeWorkerError => error
        warn "[ssr-deno] #{error.message}"
        {}
      end
    end
  end
end
