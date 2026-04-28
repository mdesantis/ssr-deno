# frozen_string_literal: true

require 'simplecov'

SimpleCov.start do
  enable_coverage :branch
  add_filter '/test_/'
end

$LOAD_PATH.unshift File.expand_path('../lib', __dir__)
require 'ssr/deno'

require 'minitest/autorun'
require 'minitest/pride' if ENV.key?('MINITEST_PRIDE')
