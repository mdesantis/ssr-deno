# frozen_string_literal: true

require 'test_helper'

module SSR
  class TestDenoConcurrency < Minitest::Test
    include TestFixturePaths

    def setup
      @bundle = SSR::Deno::Bundle.new(MINIMAL_BUNDLE)
    end

    def test_render_is_thread_safe
      n = 20
      threads = Array.new(n) { |i| Thread.new { @bundle.render({ data: { name: "T#{i}" } }) } }
      results = threads.map(&:value)

      assert_equal n, results.size
      results.each_with_index { |html, i| assert_includes html, "T#{i}" }
    end

    def test_native_render_from_ractor
      bundle_id = @bundle.instance_variable_get(:@bundle_id)

      ractor = Ractor.new(bundle_id) { |id| SSR::Deno.native_render(id, '{"data":{"name":"Ractor"}}') }
      result = ractor.respond_to?(:value) ? ractor.value : ractor.take

      assert_includes result, 'Ractor'
    end
  end
end
