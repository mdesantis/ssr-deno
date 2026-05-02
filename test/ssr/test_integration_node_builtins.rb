# frozen_string_literal: true

require 'test_helper'

SSR::Deno.render_timeout_ms = 2000
SSR::Deno.node_builtins_enabled = true

module SSR
  class TestIntegrationReactMuiEmotionSSR < Minitest::Test
    BUNDLE_PATH = File.expand_path('../../samples/react-mui-emotion-ssr-app/dist/server/entry-server.js', __dir__)

    def setup
      skip 'React MUI Emotion SSR bundle not built' unless File.exist?(BUNDLE_PATH)
    end

    def test_render_react_mui_emotion_ssr
      bundle = SSR::Deno::Bundle.new(BUNDLE_PATH)
      result = bundle.render({ data: { name: 'MUI' } })

      assert_kind_of String, result, "Expected String, got #{result.class}"
      parsed = JSON.parse(result)

      assert_includes parsed['html'], 'MUI'
      assert_includes parsed['html'], 'MuiContainer'
    end
  end

  class TestIntegrationReactMuiDashboardSSR < Minitest::Test
    BUNDLE_DIR = '../../samples/react-emotion-mui-dashboard-ssr-app/dist/server'
    BUNDLE_PATH = File.expand_path("#{BUNDLE_DIR}/entry-server.js", __dir__)

    def setup
      skip 'React MUI Dashboard SSR bundle not built' unless File.exist?(BUNDLE_PATH)
    end

    def test_render_react_mui_dashboard_ssr
      bundle = SSR::Deno::Bundle.new(BUNDLE_PATH)
      result = bundle.render({ data: { name: 'Dashboard' } })
      parsed = JSON.parse(result)

      assert_kind_of String, parsed['html']
      assert_kind_of String, parsed['css']
      assert_includes parsed['html'], 'MuiBox'
    end
  end
end
