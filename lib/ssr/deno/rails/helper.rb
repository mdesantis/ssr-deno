# frozen_string_literal: true

module SSR
  module Deno
    module RailsHelper
      # Render a named SSR bundle with given data.
      #
      # @param data [Hash, String] Data passed to the JS render function.
      #   Automatically JSON-serialized unless +raw_input: true+.
      # @param options [Hash]
      #   @option options [Symbol] :bundle  Bundle name to use
      #     (default: :application).
      #   @option options [Boolean] :raw_input  Skip JSON.generate — data is
      #     already a JSON string.
      #   @option options [Boolean] :raw_output  Skip JSON.parse — return raw
      #     JSON string from JS.
      # @return [String, Object] Raw result from the bundle. Empty string on SSR
      #   failure when +raise_on_render_error+ is false (CSR fallback).
      # @raise [SSR::Deno::BundleNotFoundError] if bundle name not registered.
      # @raise [SSR::Deno::RenderError, SSR::Deno::JsRuntimeWorkerError]
      #   when +raise_on_render_error+ is true.
      def ssr_render(data = nil, **options)
        bundle_name = options.delete(:bundle) || :application

        assert_known_ssr_render_options!(options)

        instrument 'ssr_render.ssr_deno', bundle_name: bundle_name do |payload|
          bundle = find_bundle!(bundle_name)

          bundle.render(data, **options)
        rescue SSR::Deno::RenderError, SSR::Deno::JsRuntimeWorkerError,
               SSR::Deno::JsRuntimeOutOfMemoryError => error
          payload[:error] = error.message

          fallback_or_raise(error, bundle_name, :raise_on_render_error)
        rescue SSR::Deno::BundleNotFoundError => error
          payload[:error] = error.message

          fallback_or_raise(error, bundle_name, :raise_on_bundle_error)
        end
      end

      private

      def fallback_or_raise(error, bundle_name, config_key)
        raise if Rails.application.config.ssr_deno.send(config_key)

        prefix = case error
                 in SSR::Deno::BundleNotFoundError then 'not found'
                 else 'render failed'
                 end

        Rails.logger.error "[ssr-deno] Bundle #{bundle_name.inspect} #{prefix}, " \
                           "falling back to CSR: #{error.message}"
        ''
      end

      def find_bundle!(bundle_name)
        SSR::Deno::Bundle.create_bundles!

        bundle = SSR::Deno::Bundle.registry[bundle_name]

        unless bundle
          instrument 'bundle_miss.ssr_deno', bundle_name: bundle_name

          raise SSR::Deno::BundleNotFoundError,
                "SSR bundle #{bundle_name.inspect} not registered"
        end

        bundle
      end

      def assert_known_ssr_render_options!(options)
        unknown = options.keys - %i[raw_input raw_output]

        raise ArgumentError, "unknown ssr_render option(s): #{unknown.map(&:inspect).join(', ')}" unless unknown.empty?
      end

      def instrument(...)
        Instrumenter.instrument(...)
      end
    end
  end
end
