# frozen_string_literal: true

require_relative 'deno/version'

# Load the native Rust extension (compiled by rb-sys / rake-compiler)
require_relative 'deno/ssr_deno'

module SSR
  module Deno
    class Error < StandardError; end
  end
end
