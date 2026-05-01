# frozen_string_literal: true

require 'simplecov'

SimpleCov.start do
  enable_coverage :branch
  add_filter 'test/dummy/'
  add_filter 'test/'
  add_filter 'lib/ssr/deno/rails.rb'
  add_filter 'lib/ssr/deno/rails/'
  minimum_coverage line: 100, branch: 100
  formatter SimpleCov::Formatter::MultiFormatter.new(
    [
      SimpleCov::Formatter::SimpleFormatter,
      SimpleCov::Formatter::HTMLFormatter
    ]
  )
end

$LOAD_PATH.unshift File.expand_path('../lib', __dir__)
require 'ssr/deno'

require 'minitest'
Minitest.load :profile
ARGV << '--profile'
require 'minitest/autorun'
require 'minitest/pride' if ENV.key?('MINITEST_PRIDE')
