# frozen_string_literal: true

require 'json'
require_relative 'deno/version'

# Load the native Rust extension (compiled by rb-sys / rake-compiler)
require_relative 'deno/ssr_deno'

module SSR
  module Deno
    class << self
      # Renders a component by calling the `render` function in the SSR bundle.
      #
      # @param data [Hash] Arbitrary data to pass to the render function.
      #   Will be serialized to JSON automatically.
      # @return [String] The rendered HTML string.
      # @raise [SSR::Deno::JsRuntimeNotInitializedError] if {init_runtime} has not been called
      # @raise [SSR::Deno::JsRuntimeWorkerError] if the Deno worker thread has exited
      # @raise [SSR::Deno::RenderError] if the JavaScript render function throws
      def render(data)
        native_render(JSON.generate(data))
      end
    end
  end
end
