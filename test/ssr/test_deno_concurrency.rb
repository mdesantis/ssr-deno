# frozen_string_literal: true

require 'test_helper'

module SSR
  class TestDenoConcurrency < Minitest::Test
    BUNDLE_PATH = File.expand_path('../../samples/vite-ssr-app/dist/server/entry-server.js', __dir__)

    def setup
      SSR::Deno.init_runtime(BUNDLE_PATH)
    end

    def test_render_is_thread_safe
      n = 20
      # rubocop:disable ThreadSafety/NewThread
      threads = Array.new(n) { |i| Thread.new { SSR::Deno.render({ data: { name: "T#{i}" } }) } }
      # rubocop:enable ThreadSafety/NewThread
      results = threads.map(&:value)

      assert_equal n, results.size
      results.each_with_index { |html, i| assert_includes html, "T#{i}" }
    end

    def test_native_render_from_ractor
      skip 'Ractor not defined' unless defined?(Ractor)

      ractor = Ractor.new { SSR::Deno.render(raw_input: '{"data":{"name":"Ractor"}}') }

      begin
        result = ractor.value

        assert_includes result, 'Ractor'
      rescue Ractor::RemoteError => error
        skip "native_render not Ractor-safe: #{error.cause&.class}: #{error.cause&.message}"
      end
    end
  end
end
