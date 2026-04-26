# frozen_string_literal: true

require 'test_helper'

module SSR
  class TestDeno < Minitest::Test
    def test_that_it_has_a_version_number
      refute_nil ::SSR::Deno::VERSION
    end

    def test_native_version
      assert_match(/\A\d+\.\d+\.\d+/, ::SSR::Deno.native_version)
    end

    def test_render_vite_ssr_sample
      bundle_path = File.expand_path('../../samples/vite-ssr-app/dist/server/entry-server.js', __dir__)

      assert_path_exists bundle_path, "Bundle not found at #{bundle_path}"

      # First call should return true
      result = ::SSR::Deno.init_runtime(bundle_path)

      assert(result)

      # Subsequent calls should return nil
      assert_nil ::SSR::Deno.init_runtime(bundle_path)

      html = ::SSR::Deno.render(
        component_data: { message: 'Hello World!' },
        props: {},
        url: '/'
      )

      assert_match(%r{<html>.*</html>}m, html)
      assert_includes html, 'Hello'
      assert_includes html, 'World'
      assert_includes html, '<div id="root">'
    end
  end
end
