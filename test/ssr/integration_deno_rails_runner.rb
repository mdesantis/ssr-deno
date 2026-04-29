# frozen_string_literal: true

# Boot the Rails dummy app and run integration tests.
#
# Usage:
#   BUNDLE_GEMFILE=test/dummy/Gemfile bundle exec ruby \
#     test/ssr/integration_deno_rails_runner.rb
#
# This runner is separate from the main test suite because it requires
# a full Rails application boot with its own Gemfile.

ENV['RAILS_ENV'] = 'test'

require 'simplecov'

SimpleCov.start do
  enable_coverage :branch
  add_filter 'test/dummy/'
  add_filter 'test/'
  # No minimum coverage enforcement here — this runner only exercises the
  # Rails integration subset of the codebase. The main test suite enforces
  # 100% coverage.
end

$LOAD_PATH.unshift File.expand_path('../lib', __dir__)

# Boot Rails. The dummy app's Gemfile specifies require: 'ssr/deno/rails',
# so Bundler.require will load the Rails integration (Railtie, Helper, etc.)
# after Rails framework classes are available.
require_relative '../dummy/config/environment'

require 'minitest/autorun'
require_relative 'integration_deno_rails'
