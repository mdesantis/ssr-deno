# frozen_string_literal: true

# ---------------------------------------------------------------------------
# Code coverage (must be first — hooks into Kernel#require)
# Skipped when SSR_DENO_SKIP_COVERAGE is set (e.g. performance benchmarks).
# ---------------------------------------------------------------------------

unless ENV['SSR_DENO_SKIP_COVERAGE']
  require 'simplecov'

  SimpleCov.command_name ENV.fetch('SIMPLECOV_COMMAND_NAME', 'test:main')

  SimpleCov.start do
    enable_coverage :branch
    add_filter 'test/internal/'
    add_filter 'test/dummy/'
    add_filter 'test/'
    add_filter 'lib/ssr/deno/rails.rb'
    add_filter 'lib/ssr/deno/rails/'
    add_filter 'lib/ssr/deno/ractor_pool' # SimpleCov can't trace inside Ractors
    formatter SimpleCov::Formatter::MultiFormatter.new(
      [
        SimpleCov::Formatter::SimpleFormatter,
        SimpleCov::Formatter::HTMLFormatter
      ]
    )
  end
end

# ---------------------------------------------------------------------------
# Library under test
# ---------------------------------------------------------------------------
# Config defaults are set in each runner script (tmp/test_runner_*.rb)
# to allow different suites to use different settings.
# test:main runner sets isolate_pool_size=1 (default).
$LOAD_PATH.unshift File.expand_path('../lib', __dir__)
require 'ssr/deno'

# ---------------------------------------------------------------------------
# Shared support modules
# ---------------------------------------------------------------------------
require 'support/fixture_paths'
require 'support/temp_bundle_helper'

# ---------------------------------------------------------------------------
# Test framework
# ---------------------------------------------------------------------------

require 'minitest'
Minitest.load :profile
Warning[:experimental] = false
ARGV << '--profile'
require 'minitest/autorun'
require 'minitest/pride' if %w[true yes 1].include?(ENV['MINITEST_PRIDE']&.downcase)
