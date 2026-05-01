# frozen_string_literal: true

require 'test_helper'

module SSR
  class TestIntegrationViteSsr < Minitest::Test
    BUNDLE_PATH = File.expand_path('../../samples/vite-ssr-app/dist/server/entry-server.js', __dir__)

    def setup
      skip 'Vite SSR bundle not built — run `bundle exec rake samples:build`' unless File.exist?(BUNDLE_PATH)
    end

    def test_render_produces_valid_html
      bundle = SSR::Deno::Bundle.new(BUNDLE_PATH)
      html = bundle.render({ data: { name: 'Integration' } })

      assert_match(%r{<html>.*</html>}m, html)
      assert_includes html, 'Integration'
      assert_includes html, '<div id="root">'
    end
  end
end
