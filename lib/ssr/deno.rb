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
      # @param data [Hash, String] Data to pass to the render function. When
      #   +raw_input: true+, must be a pre-serialized JSON string.
      # @param raw_input [Boolean] Skip +JSON.generate+ — data is already a JSON string.
      # @param raw_output [Boolean] Skip +JSON.parse+ — return the raw JSON string.
      # @return [String, Hash, Array, Numeric, Boolean, nil] Deserialized return
      #   value from the JavaScript `render` function, or a raw JSON String when
      #   +raw_output: true+.
      # @raise [SSR::Deno::JsRuntimeNotInitializedError] if {init_runtime} has not been called
      # @raise [SSR::Deno::JsRuntimeWorkerError] if the Deno worker thread has exited
      # @raise [SSR::Deno::RenderError] if the JavaScript render function throws
      def render(data, raw_input: false, raw_output: false)
        json_input = raw_input ? data : JSON.generate(data)
        result = native_render(json_input)
        raw_output ? result : JSON.parse(result)
      end
    end
  end
end
