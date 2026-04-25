# frozen_string_literal: true

require 'test_helper'

module SSR
  class TestDeno < Minitest::Test
    def test_that_it_has_a_version_number
      refute_nil ::SSR::Deno::VERSION
    end
  end
end
