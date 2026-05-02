# frozen_string_literal: true

require 'test_helper'

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
end
