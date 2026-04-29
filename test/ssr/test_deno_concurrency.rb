# frozen_string_literal: true

require 'test_helper'

module SSR
  class TestDenoConcurrency < Minitest::Test
    BUNDLE_PATH = File.expand_path('../../samples/vite-ssr-app/dist/server/entry-server.js', __dir__)

    def setup
      @bundle = SSR::Deno::Bundle.new(BUNDLE_PATH)
    end

    def test_render_is_thread_safe
      n = 20
      # rubocop:disable ThreadSafety/NewThread
      threads = Array.new(n) { |i| Thread.new { @bundle.render({ data: { name: "T#{i}" } }) } }
      # rubocop:enable ThreadSafety/NewThread
      results = threads.map(&:value)

      assert_equal n, results.size
      results.each_with_index { |html, i| assert_includes html, "T#{i}" }
    end

    def test_native_render_from_ractor
      bundle_id = @bundle.instance_variable_get(:@bundle_id)
      prev_experimental = Warning[:experimental]
      Warning[:experimental] = false
      ractor = Ractor.new(bundle_id) { |id| SSR::Deno.native_render(id, '{"data":{"name":"Ractor"}}') }
      Warning[:experimental] = prev_experimental
      result = ractor.value

      assert_includes result, 'Ractor'
    end
  end
end
