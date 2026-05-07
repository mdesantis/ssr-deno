# frozen_string_literal: true

unless ENV['SSR_DENO_SKIP_COVERAGE']
  require 'simplecov'

  SimpleCov.command_name ENV.fetch('SIMPLECOV_COMMAND_NAME', 'test:rails')

  SimpleCov.start do
    enable_coverage :branch
    add_filter 'test/internal/'
    add_filter 'test/'
    add_filter 'lib/ssr/deno/rails.rb'
    add_filter 'lib/ssr/deno/rails/'
    formatter SimpleCov::Formatter::MultiFormatter.new(
      [
        SimpleCov::Formatter::SimpleFormatter,
        SimpleCov::Formatter::HTMLFormatter
      ]
    )
  end
end

$LOAD_PATH.unshift File.expand_path('../lib', __dir__)

require 'rails'
require 'ssr/deno/rails'

require 'combustion'

Combustion.path = 'test/internal'
Combustion.initialize! :action_view, :action_controller

require 'minitest'
Minitest.load :profile
Warning[:experimental] = false
ARGV << '--profile'

require 'minitest/autorun'
require 'minitest/pride' if %w[true yes 1].include?(ENV['MINITEST_PRIDE']&.downcase)
