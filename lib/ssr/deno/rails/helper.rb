# frozen_string_literal: true

module SSR
  module Deno
    module Helper
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
      # @return [String] Rendered output (html_safe). Empty string on SSR
      #   failure when +raise_on_render_error+ is false (CSR fallback).
      # @raise [SSR::Deno::BundleNotFoundError] if bundle name not registered.
      # @raise [SSR::Deno::RenderError, SSR::Deno::JsRuntimeWorkerError]
      #   when +raise_on_render_error+ is true.
      def ssr_render(data = nil, **options)
        bundle_name = options.delete(:bundle) || :application
        bundle = find_bundle!(bundle_name)

        bundle.render(data, **options).html_safe
      rescue SSR::Deno::RenderError, SSR::Deno::JsRuntimeWorkerError,
             SSR::Deno::JsRuntimeOutOfMemoryError => error
        raise if Rails.application.config.ssr_deno.raise_on_render_error

        Rails.logger.error "[ssr-deno] Bundle #{bundle_name.inspect} render failed, " \
                           "falling back to CSR: #{error.message}"
        ''.html_safe
      end

      private

      def find_bundle!(bundle_name)
        bundle = SSR::Deno::Bundle.registry[bundle_name]

        unless bundle.is_a?(SSR::Deno::Bundle)
          SSR::Deno::Bundle.create_bundles!
          bundle = SSR::Deno::Bundle.registry[bundle_name]
        end

        unless bundle.is_a?(SSR::Deno::Bundle)
          instrument 'bundle_miss.ssr_deno', bundle_name: bundle_name

          raise SSR::Deno::BundleNotFoundError,
                "SSR bundle #{bundle_name.inspect} not registered"
        end

        bundle
      end

      def instrument(name, payload = {})
        return unless defined?(ActiveSupport::Notifications)

        ActiveSupport::Notifications.instrument(name, payload)
      end
    end
  end
end
