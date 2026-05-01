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
      # Must be called before any Bundle.new call (triggers pool init).
      #
      # max_heap_size_mb is a per-isolate V8 CreateParams constraint, NOT a
      # total process budget. Each isolate independently gets this limit,
      # regardless of pool size. This ensures workloads calibrated for a
      # single isolate don't break when the pool auto-detects more cores.
      #
      # Default: 64 MB — sensible for typical SSR workloads (~20 MB baseline +
      # bundle + render peak + headroom). Set to 0 for unlimited (V8 built-in
      # default, ~1.4 GB on 64-bit).
      #
      # @param mega_bytes [Integer] heap size in MB
      def max_heap_size_mb=(mega_bytes)
        native_set_max_heap_size_mb(mega_bytes.to_i)
      end

      # Set the number of V8 isolates in the pool before initializing the runtime.
      # Must be called before any Bundle.new call (triggers pool init).
      #
      # Default: 0 = auto-detect from CPU count (capped at 8, minus one for
      # the Ruby thread). A pool of 2–4 is typical for concurrent SSR.
      #
      # @param size [Integer] isolate count (0 = auto-detect, min 1, max 8)
      def isolate_pool_size=(size)
        native_set_isolate_pool_size(size.to_i)
      end
    end
  end
end
