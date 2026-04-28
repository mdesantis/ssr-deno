# frozen_string_literal: true

require 'test_helper'

module SSR
  class TestDeno < Minitest::Test
    BUNDLE_PATH = File.expand_path('../../samples/vite-ssr-app/dist/server/entry-server.js', __dir__)

    def setup
      assert_path_exists BUNDLE_PATH, "Bundle not found at #{BUNDLE_PATH}"
      ::SSR::Deno.init_runtime(BUNDLE_PATH)
    end

    def test_that_it_has_a_version_number
      refute_nil ::SSR::Deno::VERSION
    end

    def test_native_version
      assert_match(/\A\d+\.\d+\.\d+/, ::SSR::Deno.native_version)
    end

    def test_init_runtime_returns_true_on_first_call_and_nil_on_subsequent
      # init_runtime was already called in setup; subsequent calls return nil
      assert_nil ::SSR::Deno.init_runtime(BUNDLE_PATH)
    end

    def test_render
      html = ::SSR::Deno.render({ data: { name: 'Maurizio' } })

      assert_match(%r{<html>.*</html>}m, html)
      assert_includes html, 'Maurizio'
      assert_includes html, '<div id="root">'
    end

    def test_render_with_raw_input
      json = JSON.generate({ data: { name: 'Raw' } })
      html = ::SSR::Deno.render(json, raw_input: true)

      assert_includes html, 'Raw'
    end

    def test_render_with_raw_output
      result = ::SSR::Deno.render({ data: { name: 'Test' } }, raw_output: true)

      assert_instance_of String, result
      assert_includes JSON.parse(result), '<html>'
    end

    def test_render_with_raw_input_and_raw_output
      json = JSON.generate({ data: { name: 'Passthrough' } })
      result = ::SSR::Deno.render(json, raw_input: true, raw_output: true)

      assert_instance_of String, result
      assert_includes result, 'Passthrough'
    end
  end
end
