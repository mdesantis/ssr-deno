# frozen_string_literal: true

$LOAD_PATH.unshift File.expand_path('../lib', __dir__)
require 'ssr/deno'

require 'minitest/autorun'
require 'minitest/pride' if ENV.key?('MINITEST_PRIDE')
