# frozen_string_literal: true

require_relative 'bundle/registry'
require_relative 'instrumenter'
require_relative 'manifest'

module SSR
  module Deno
    class Bundle
      class << self
        attr_reader :registry
      end

      @registry = Registry.new

      # @param bundle_path [String] Path to the SSR bundle (entry-server.js)
      # @param manifest_path [String, nil] Path to Vite client manifest
      #   (.vite/manifest.json). When provided, enables asset discovery
      #   via the +assets+ method.
      # @param client_entry [String, nil] Source entry key in the manifest
      #   (e.g., "src/entry-client.ts"). Required when manifest_path is given.
      def initialize(bundle_path, manifest_path: nil, client_entry: nil)
        @bundle_path = bundle_path.to_s
        @bundle_id = "#{File.basename(@bundle_path)}##{object_id}"
        @mtime = File.mtime(@bundle_path)
        @auto_reload = false

        if manifest_path
          @manifest = Manifest.new(manifest_path)
          @client_entry = client_entry || raise(ArgumentError, 'client_entry required when manifest_path is provided')
        end

        instrument 'bundle_load.ssr_deno', bundle_name: @bundle_id, path: @bundle_path do
          load
        end
      end

      # Enable or disable auto-reload (mtime check before each render).
      # @param value [Boolean]
      attr_writer :auto_reload

      # Reload the bundle from disk. Called automatically when +auto_reload+
      # is enabled and the file mtime has changed.
      def reload
        @mtime = File.mtime(@bundle_path)

        instrument 'bundle_load.ssr_deno', bundle_name: @bundle_id, path: @bundle_path do
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
      #   the process with SIGTRAP. See {file:plans/archived/v8-oom-protection.md}.
      def render(data = nil, raw_input: false, raw_output: false)
        reload_if_changed if @auto_reload

        json_input = raw_input ? data : JSON.generate(data)

        instrument 'render.ssr_deno', bundle_name: @bundle_id do
          result = SSR::Deno.native_render(@bundle_id, json_input)

          raw_output ? result : JSON.parse(result)
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

        SSR::Deno.native_render_chunks(@bundle_id, json_input, &)
      end

      # Returns a hash of all discovered client-side assets when a
      # Vite manifest is configured. Returns an empty hash otherwise.
      #
      # @param prefix [String] URL prefix for asset paths (default: "/")
      # @return [Hash] Asset info with keys: :css_tags, :client_js_tag, :asset_urls
      def assets(prefix: '/')
        return {} unless @manifest

        @manifest.assets(@client_entry, prefix:)
      end

      # Returns HTML `<link>` tags for all CSS files.
      #
      # @param prefix [String] URL prefix (default: "/")
      # @return [String] HTML `<link>` tags, or empty string if no manifest
      def css_tags(prefix: '/')
        return '' unless @manifest

        @manifest.css_tags(@client_entry, prefix:)
      end

      # Returns the `<script>` tag for the client JS entry point.
      #
      # @param prefix [String] URL prefix (default: "/")
      # @return [String] HTML `<script type="module">` tag, or empty string
      def client_js_tag(prefix: '/')
        return '' unless @manifest

        @manifest.client_js_tag(@client_entry, prefix:)
      end

      # Returns URLs for all static asset files (images, fonts, etc.).
      #
      # @param prefix [String] URL prefix (default: "/")
      # @return [Array<String>] Asset URL paths, or empty array
      def asset_urls(prefix: '/')
        return [] unless @manifest

        @manifest.asset_urls(@client_entry, prefix:)
      end

      private

      # Delegate instrumentation to the shared Instrumenter module.
      def instrument(...)
        Instrumenter.instrument(...)
      end

      # Load (or reload) the bundle into the Deno runtime.
      def load
        SSR::Deno.native_load_bundle(@bundle_id, @bundle_path)
      end

      # Reload the bundle if the file has changed on disk.
      def reload_if_changed
        current_mtime = File.mtime(@bundle_path)

        return unless current_mtime > @mtime

        reload
      end
    end
  end
end
