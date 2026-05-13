# frozen_string_literal: true

require 'json'

module SSR
  module Deno
    # Dev-mode bundle that loads source `.tsx` files directly into an embedded
    # Deno V8 isolate via the DevModuleLoader. No pre-build step required.
    #
    # Registers itself in {Bundle.registry} so {RailsHelper#find_bundle!}
    # resolves it transparently (same `#render` / `#render_chunks` interface).
    #
    # @note Dev-mode only. Requires the native extension compiled with
    #   `--features dev-mode`. Raises NoMethodError at runtime otherwise.
    class DevModeBundle
      # @param entry_path [String] Path to the source entry file (.tsx/.ts).
      # @param name [Symbol, String] Registry name for +find_bundle!+ lookup.
      #   Defaults to +entry_path+.
      # @param resolve_alias [Hash{String => String}] Path alias map, e.g.
      #   `{ '@' => 'app/frontend' }`.  Keys and values are converted to
      #   strings.  Defaults to {Config.dev_resolve_alias}.
      # @param project_root [String] Project root directory for permission
      #   boundary and `node_modules/` resolution.  Defaults to +Dir.pwd+.
      def initialize(entry_path, name: nil, resolve_alias: nil,
                     project_root: Dir.pwd)
        @entry_path = entry_path.to_s
        @resolve_alias = (resolve_alias || SSR::Deno::Config.dev_resolve_alias)
                         .transform_keys(&:to_s).transform_values(&:to_s)
        @project_root = project_root.to_s
        @name = name || @entry_path

        create_worker
        load_entry

        Bundle.registry[@name] = self
      end

      # Render the entry via the dev worker's full Deno event loop.
      # @param data [Hash, String] Data passed to the JS render function.
      # @param raw_input [Boolean] Skip +JSON.generate+.
      # @param raw_output [Boolean] Skip +JSON.parse+.
      def render(data = nil, raw_input: false, raw_output: false)
        json = raw_input ? data : JSON.generate(data)

        result = SSR::Deno.native_dev_render(@handle, @entry_path, json)

        raw_output ? result : JSON.parse(result)
      end

      # Chunked render. Yields HTML chunks incrementally.
      # @param data [Hash, String] Data passed to the JS render function.
      # @param raw_input [Boolean] Skip +JSON.generate+.
      # @return [Enumerator, nil] Enumerator of chunks (no block) or nil (block given).
      def render_chunks(data = nil, raw_input: false, &)
        json = raw_input ? data : JSON.generate(data)

        SSR::Deno.native_dev_render_chunks(@handle, @entry_path, json, &)
      end

      private

      def create_worker
        @handle = SSR::Deno.native_dev_worker_new(
          @project_root,
          SSR::Deno::Config.max_heap_size_mb,
          SSR::Deno::Config.render_timeout_ms
        )
      end

      def load_entry
        SSR::Deno.native_dev_load_entry(
          @handle,
          @entry_path,
          JSON.generate(@resolve_alias)
        )
      end
    end
  end
end
