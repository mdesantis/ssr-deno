# frozen_string_literal: true

# ---------------------------------------------------------------------------
# Code coverage (must be first — hooks into Kernel#require)
# ---------------------------------------------------------------------------

require 'simplecov'

SimpleCov.command_name ENV.fetch('SIMPLECOV_COMMAND_NAME', 'test:main')

SimpleCov.start do
  enable_coverage :branch
  add_filter 'test/dummy/'
  add_filter 'test/'
  add_filter 'lib/ssr/deno/rails.rb'
  add_filter 'lib/ssr/deno/rails/'
  # Coverage thresholds are enforced on the merged report
  # (both test:main and test:node_builtins combined).
  formatter SimpleCov::Formatter::MultiFormatter.new(
    [
      SimpleCov::Formatter::SimpleFormatter,
      SimpleCov::Formatter::HTMLFormatter
    ]
  )
end

# ---------------------------------------------------------------------------
# Library under test
# ---------------------------------------------------------------------------

$LOAD_PATH.unshift File.expand_path('../lib', __dir__)
require 'ssr/deno'

SSR::Deno.isolate_pool_size = 1

# ---------------------------------------------------------------------------
# Test framework
# ---------------------------------------------------------------------------

require 'minitest'
Minitest.load :profile
ARGV << '--profile'
require 'minitest/autorun'
require 'minitest/pride' if %w[true yes 1].include?(ENV['MINITEST_PRIDE']&.downcase)
