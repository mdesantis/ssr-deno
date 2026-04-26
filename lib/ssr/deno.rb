# frozen_string_literal: true

require 'json'
require_relative 'deno/version'

# Load the native Rust extension (compiled by rb-sys / rake-compiler)
require_relative 'deno/ssr_deno'

module SSR
  module Deno
    class Error < StandardError; end

    class << self
      # Renders a component by calling the `render` function in the SSR bundle.
      #
      # @param data [Hash] Arbitrary data to pass to the render function.
      #   Will be serialized to JSON automatically.
      # @return [String] The rendered HTML string.
      def render(data)
        native_render(JSON.generate(data))
      end
    end
  end
end
