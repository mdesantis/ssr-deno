# frozen_string_literal: true

require 'test_helper'

module SSR
  class TestDenoBundle < Minitest::Test
    BUNDLE_PATH = File.expand_path('../../samples/vite-ssr-app/dist/server/entry-server.js', __dir__)

    def setup
      assert_path_exists BUNDLE_PATH, "Bundle not found at #{BUNDLE_PATH}"
      @bundle = SSR::Deno::Bundle.new(BUNDLE_PATH)
    end

    def test_render
      html = @bundle.render({ data: { name: 'Maurizio' } })

      assert_match(%r{<html>.*</html>}m, html)
      assert_includes html, 'Maurizio'
      assert_includes html, '<div id="root">'
    end

    def test_render_with_raw_input
      json = JSON.generate({ data: { name: 'Raw' } })
      html = @bundle.render(json, raw_input: true)

      assert_includes html, 'Raw'
    end

    def test_render_with_raw_output
      result = @bundle.render({ data: { name: 'Test' } }, raw_output: true)

      assert_instance_of String, result
      assert_includes JSON.parse(result), '<html>'
    end

    def test_render_with_raw_input_and_raw_output
      json = JSON.generate({ data: { name: 'Passthrough' } })
      result = @bundle.render(json, raw_input: true, raw_output: true)

      assert_instance_of String, result
      assert_includes result, 'Passthrough'
    end

    def test_multiple_bundles_coexist
      bundle_b = SSR::Deno::Bundle.new(BUNDLE_PATH)

      html_a = @bundle.render({ data: { name: 'Alice' } })
      html_b = bundle_b.render({ data: { name: 'Bob' } })

      assert_includes html_a, 'Alice'
      assert_includes html_b, 'Bob'
    end

    def test_render_with_custom_fn_name
      html = @bundle.render({ data: { name: 'Custom' } }, fn_name: 'renderSSR')

      assert_includes html, 'Custom'
    end

    def test_bundle_not_found_raises_bundle_not_found_error
      assert_raises(SSR::Deno::BundleNotFoundError) do
        SSR::Deno.native_render('nonexistent_bundle_id', 'render', '{}')
      end
    end
  end
end
