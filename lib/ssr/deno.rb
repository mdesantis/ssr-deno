# frozen_string_literal: true

require 'json'
require_relative 'deno/version'

# Load the native Rust extension (compiled by rb-sys / rake-compiler)
require_relative 'deno/ssr_deno'
require_relative 'deno/instrumenter'
require_relative 'deno/bundle'

module SSR
  module Deno
    class << self
      # Set the maximum V8 heap size in megabytes before initializing the runtime.
      # Must be called before any Bundle.new call (triggers OnceLock runtime init).
      #
      # Default: 64 MB — sensible for typical SSR workloads (~20 MB baseline +
      # bundle + render peak + headroom). Set to 0 for unlimited (V8 built-in
      # default, ~1.4 GB on 64-bit).
      #
      # @param mega_bytes [Integer] heap size in MB
      def max_heap_size_mb=(mega_bytes)
        native_set_max_heap_size_mb(mega_bytes.to_i)
      end
    end
  end
end
