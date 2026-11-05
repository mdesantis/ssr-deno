# frozen_string_literal: true

require 'json'

require_relative 'instrumenter'

module SSR
  module Deno
    class Bundle
      class << self
        attr_reader :registry

        # Create bundles from config hashes stored in +registry+.
        # Thread-safe (outer guard + mutex). Call from +on_worker_boot+
        # (Puma clustered) or lazily on first render (single mode).
        #
        # Entries whose file does not exist yet stay as config hashes rather
        # than raising — a bundle may be built after boot (e.g. by a
        # +test:prepare+ hook). {fetch} instantiates them once the file
        # appears, and raises +BundleNotFoundError+ until then.
        # rubocop:disable-next ThreadSafety/ClassInstanceVariable
        def create_bundles!
          return if @_bundles_created

          @_create_mutex.synchronize do
            next if @_bundles_created

            # Walk a snapshot: DevModeBundle#initialize may add a key from
            # another thread mid-walk, and live iteration would make that
            # write raise. Keys added later are materialized by fetch.
            @registry.keys.each { |name| materialize(name) } # rubocop:disable Style/HashEachMethods -- snapshot is the point

            @_bundles_created = true
          end
        end

        # Look up a named bundle, instantiating its config hash on demand.
        #
        # @param name [Symbol, String] registry key
        # @return [Bundle, DevModeBundle, nil] nil when +name+ is not registered
        # @raise [SSR::Deno::BundleNotFoundError] if +name+ is registered but
        #   its file does not exist (yet)
        def fetch(name)
          create_bundles!

          # Unsynchronized read on the render hot path. Writers can still run
          # concurrently — materialize swaps values under the mutex, and
          # DevModeBundle#initialize adds keys without it — which MRI's GVL
          # makes safe. A stale config hash is re-checked under the mutex.
          entry = @registry[name] # rubocop:disable ThreadSafety/ClassInstanceVariable

          return entry unless entry.is_a?(Hash)

          @_create_mutex.synchronize do
            materialize(name) or
              raise BundleNotFoundError, "SSR bundle #{name.inspect} not found at #{@registry[name][:path]}"
          end
        end

        private

        # Replace the config hash registered as +name+ with a +Bundle+ when its
        # file exists. Entries that are already instantiated bundles — e.g. a
        # SSR::Deno::DevModeBundle registered itself directly — are returned
        # as-is. Caller must hold +@_create_mutex+.
        #
        # @return [Bundle, DevModeBundle, nil] nil when the file is missing
        # rubocop:disable-next ThreadSafety/ClassInstanceVariable -- caller holds the mutex
        def materialize(name)
          cfg = @registry[name]

          return cfg unless cfg.is_a?(Hash)
          return unless File.exist?(cfg[:path])

          bundle = new(cfg[:path])

          bundle.auto_reload = true if cfg[:auto_reload]

          @registry[name] = bundle
        end
      end

      @registry = {} # rubocop:disable ThreadSafety/MutableClassInstanceVariable
      @_create_mutex = Mutex.new
      @_bundles_created = false

      # @param bundle_path [String] Path to the SSR bundle (entry-server.js)
      #
      # Two Bundle instances with the same path share the same native bundle_id.
      # The second #load overwrites the first in the Rust layer — intentional
      # file-level deduplication.
      def initialize(bundle_path)
        @bundle_path = bundle_path.to_s
        @mtime = File.mtime(@bundle_path)
        @auto_reload = false
        @_bundle_mutex = Mutex.new

        instrument 'bundle_load.ssr_deno',
                   bundle_name: @bundle_path, path: @bundle_path, identifier: @bundle_path do
          load
        end
      end

      # @return [Boolean] Reader is unsynchronized — benign on MRI (GVL),
      #   atomic in practice on JRuby/TruffleRuby (single-word write).
      attr_reader :auto_reload, :bundle_path

      # Enable or disable auto-reload (mtime check before each render).
      # @param value [Boolean]
      def auto_reload=(value)
        @_bundle_mutex.synchronize { @auto_reload = value }
      end

      # Reload the bundle from disk. Called automatically when +auto_reload+
      # is enabled and the file mtime has changed.
      def reload
        @_bundle_mutex.synchronize do
          @mtime = File.mtime(@bundle_path)
        end

        instrument 'bundle_load.ssr_deno',
                   bundle_name: @bundle_path, path: @bundle_path, identifier: @bundle_path do
          load
        end
      end

      # Render the bundle with the full Deno event loop. Macrotasks like
      # +setTimeout+, +setInterval+, and +MessageChannel+ fire normally.
      #
      # @param data [Hash, String] Data to pass to the render function.
      #   When +raw_input: true+, must be a pre-serialized JSON string.
      # @param raw_input [Boolean] Skip +JSON.generate+ -- data is already a JSON string.
      # @param raw_output [Boolean] Skip +JSON.parse+ -- return the raw JSON string.
      # @return [String, Hash, Array, Numeric, Boolean, nil] Deserialized return
      #   value from the JavaScript `render` function, or a raw JSON String when
      #   +raw_output: true+.
      # @raise [SSR::Deno::BundleNotFoundError] if the bundle was not loaded
      # @raise [SSR::Deno::JsRuntimeWorkerError] if the Deno worker thread has exited
      # @raise [SSR::Deno::RenderError] if the JavaScript render function throws
      # @raise [SSR::Deno::JsRuntimeOutOfMemoryError] if the V8 isolate heap
      #   exceeds the configured limit (+max_heap_size_mb+). A user component
      #   that allocates memory across renders (leaks) can trigger this. The
      #   near-heap-limit callback terminates execution before V8 would crash
      #   the process with SIGTRAP. See https://github.com/mdesantis/ssr-deno/blob/main/plans/archived/v8-oom-protection.md.
      def render(data = nil, raw_input: false, raw_output: false)
        reload_if_changed if @auto_reload

        json_input = raw_input ? data : JSON.generate(data)

        instrument 'render.ssr_deno', bundle_name: @bundle_path, identifier: @bundle_path do |payload|
          result = SSR::Deno.native_render(@bundle_path, json_input)

          raw_output ? result : JSON.parse(result)
        rescue StandardError => error
          payload[:error] = error.message

          raise
        end
      end

      # Chunked render -- yields HTML chunks incrementally as they arrive from
      # the JS render function (e.g., React's +renderToPipeableStream+ or any
      # renderer calling +globalThis.__ssr_push_chunk(string)+).
      #
      # Returns an +Enumerator+ when no block is given (Rack 3 compatible --
      # usable directly as a response body). When a block IS given, yields each
      # chunk to the block and raises on error.
      #
      # @param data [Hash, String] Data to pass to the render function.
      #   When +raw_input: true+, must be a pre-serialized JSON string.
      # @param raw_input [Boolean] Skip +JSON.generate+ -- data is already a JSON string.
      # @return [Enumerator, nil] Enumerator of HTML chunk strings (no block),
      #   or nil (block given, chunks yielded).
      # @raise [SSR::Deno::BundleNotFoundError] if the bundle was not loaded
      # @raise [SSR::Deno::JsRuntimeWorkerError] if the Deno worker thread has exited
      # @raise [SSR::Deno::RenderError] if the JavaScript render function throws
      # @raise [SSR::Deno::JsRuntimeOutOfMemoryError] if the V8 isolate heap
      #   exceeds the configured limit (+max_heap_size_mb+)
      def render_chunks(data = nil, raw_input: false, &)
        reload_if_changed if @auto_reload

        json_input = raw_input ? data : JSON.generate(data)

        # Block form only: the Enumerator form returns immediately and raises
        # later, at iteration time, by which point this block — and the
        # instrumentation event — has already completed successfully.
        instrument 'render.ssr_deno', bundle_name: @bundle_path, identifier: @bundle_path do |payload|
          SSR::Deno.native_render_chunks(@bundle_path, json_input, &)
        rescue StandardError => error
          payload[:error] = error.message

          raise
        end
      end

      private

      # Delegate instrumentation to the shared Instrumenter module.
      def instrument(...)
        Instrumenter.instrument(...)
      end

      # Load (or reload) the bundle into the Deno runtime.
      def load
        SSR::Deno.native_load_bundle(@bundle_path, @bundle_path)
      end

      # Reload the bundle if the file has changed on disk.
      # Thread-safe (@_bundle_mutex guards @mtime read-check-write).
      def reload_if_changed
        needs_reload = @_bundle_mutex.synchronize do
          current_mtime = File.mtime(@bundle_path)

          if current_mtime > @mtime
            @mtime = current_mtime

            true
          else
            false
          end
        end

        reload if needs_reload
      end
    end
  end
end
