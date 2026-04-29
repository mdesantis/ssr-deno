# frozen_string_literal: true

require_relative 'bundle/registry'

module SSR
  module Deno
    class Bundle
      class << self
        attr_reader :registry
      end

      @registry = Registry.new

      # @param bundle_path [String] Path to the Vite SSR bundle (entry-server.js)
      def initialize(bundle_path)
        @bundle_path = bundle_path.to_s
        @bundle_id = object_id.to_s
        @mtime = File.mtime(@bundle_path)
        @auto_reload = false

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

      # @param data [Hash, String] Data to pass to the render function.
      #   When +raw_input: true+, must be a pre-serialized JSON string.
      # @param raw_input [Boolean] Skip +JSON.generate+ — data is already a JSON string.
      # @param raw_output [Boolean] Skip +JSON.parse+ — return the raw JSON string.
      # @return [String, Hash, Array, Numeric, Boolean, nil] Deserialized return
      #   value from the JavaScript `render` function, or a raw JSON String when
      #   +raw_output: true+.
      # @raise [SSR::Deno::BundleNotFoundError] if the bundle was not loaded
      # @raise [SSR::Deno::JsRuntimeWorkerError] if the Deno worker thread has exited
      # @raise [SSR::Deno::RenderError] if the JavaScript render function throws
      def render(data = nil, raw_input: false, raw_output: false)
        reload_if_changed if @auto_reload

        json_input = raw_input ? data : JSON.generate(data)

        instrument 'render.ssr_deno', bundle_name: @bundle_id do
          result = SSR::Deno.native_render(@bundle_id, json_input)
          raw_output ? result : JSON.parse(result)
        end
      end

      private

      # Instrument a block with ActiveSupport::Notifications.
      # No-ops when ActiveSupport::Notifications is not loaded (core gem mode).
      # :nocov: — the ActiveSupport::Notifications branch is exercised by Rails
      # integration tests (test/ssr/integration_deno_rails.rb), which are
      # excluded from SimpleCov because they require a full Rails boot.
      def instrument(name, payload = {}, &)
        return yield unless defined?(ActiveSupport::Notifications)

        ActiveSupport::Notifications.instrument(name, payload, &)
      end
      # :nocov:

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
