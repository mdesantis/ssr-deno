# frozen_string_literal: true

require_relative 'deno/version'

# Load the native Rust extension (compiled by rb-sys / rake-compiler)
require_relative 'deno/ssr_deno'
require_relative 'deno/render_error'
require_relative 'deno/heap_stats'
require_relative 'deno/config'
require_relative 'deno/instrumenter'
require_relative 'deno/bundle'
require_relative 'deno/dev_mode_bundle'
require_relative 'deno/ractor_pool'

module SSR
  module Deno
  end
end
