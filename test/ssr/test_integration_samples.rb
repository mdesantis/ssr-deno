# frozen_string_literal: true

require 'test_helper'

module SSR
  class TestIntegrationReactSSR < Minitest::Test
    BUNDLE_PATH = File.expand_path('../../samples/vite-react-ssr-app/dist/server/entry-server.js', __dir__)

    def setup
      skip 'React SSR bundle not built — run `bundle exec rake samples:build`' unless File.exist?(BUNDLE_PATH)
    end

    def test_render_produces_valid_html
      bundle = SSR::Deno::Bundle.new(BUNDLE_PATH)
      html = bundle.render({ data: { name: 'Integration' } })

      assert_match(%r{<html>.*</html>}m, html)
      assert_includes html, 'Integration'
      assert_includes html, '<div id="root">'
    end
  end

  class TestIntegrationVanillaSSR < Minitest::Test
    BUNDLE_PATH = File.expand_path('../../samples/vite-ssr-app/dist/server/entry-server.js', __dir__)

    def setup
      skip 'Vanilla SSR bundle not built — run `bundle exec rake samples:build`' unless File.exist?(BUNDLE_PATH)
    end

    def test_render_vanilla_ssr
      bundle = SSR::Deno::Bundle.new(BUNDLE_PATH)
      html = bundle.render({ name: 'Vanilla' })

      assert_match(%r{<html>.*</html>}m, html)
      assert_includes html, 'Vanilla'
      assert_includes html, '<div id="root">'
    end
  end

  class TestIntegrationSvelteSSR < Minitest::Test
    BUNDLE_PATH = File.expand_path('../../samples/vite-svelte-ssr-app/dist/server/entry-server.js', __dir__)

    def setup
      skip 'Svelte SSR bundle not built — run `bundle exec rake samples:build`' unless File.exist?(BUNDLE_PATH)
    end

    def test_render_svelte_ssr
      bundle = SSR::Deno::Bundle.new(BUNDLE_PATH)
      html = bundle.render({ data: { name: 'Svelte' } })

      assert_match(%r{<html>.*</html>}m, html)
      assert_includes html, 'Svelte'
      assert_includes html, '<div id="root">'
    end
  end

  class TestIntegrationReactMuiSSR < Minitest::Test
    BUNDLE_PATH = File.expand_path('../../samples/vite-react-mui-ssr-app/dist/server/entry-server.js', __dir__)

    def setup
      skip 'React MUI SSR bundle not built — run `bundle exec rake samples:build`' unless File.exist?(BUNDLE_PATH)
    end

    def test_render_react_mui_ssr
      bundle = SSR::Deno::Bundle.new(BUNDLE_PATH)
      html = bundle.render({ data: { name: 'MUI' } })

      assert_match(%r{<html>.*</html>}m, html)
      assert_includes html, 'MUI'
      assert_includes html, 'MuiContainer'
    end
  end

  class TestIntegrationVueSSR < Minitest::Test
    BUNDLE_PATH = File.expand_path('../../samples/vite-vue-ssr-app/dist/server/entry-server.js', __dir__)

    def setup
      skip 'Vue SSR bundle not built — run `bundle exec rake samples:build`' unless File.exist?(BUNDLE_PATH)
    end

    def test_render_vue_ssr
      bundle = SSR::Deno::Bundle.new(BUNDLE_PATH)
      html = bundle.render({ data: { name: 'Vue' } })

      assert_match(%r{<html>.*</html>}m, html)
      assert_includes html, 'Vue'
    end
  end

  class TestIntegrationBareboneSSR < Minitest::Test
    BUNDLE_PATH = File.expand_path('../../samples/barebone-ssr-app/entry-server.js', __dir__)

    def test_render_barebone
      bundle = SSR::Deno::Bundle.new(BUNDLE_PATH)
      html = bundle.render({ name: 'Barebone' })

      assert_match(%r{<html>.*</html>}m, html)
      assert_includes html, 'Barebone'
      assert_includes html, '<div id="root">'
    end
  end

  class TestIntegrationPreactSSR < Minitest::Test
    BUNDLE_PATH = File.expand_path('../../samples/vite-preact-ssr-app/dist/server/entry-server.js', __dir__)

    def setup
      skip 'Preact SSR bundle not built — run `bundle exec rake samples:build`' unless File.exist?(BUNDLE_PATH)
    end

    def test_render_preact_ssr
      bundle = SSR::Deno::Bundle.new(BUNDLE_PATH)
      html = bundle.render({ data: { name: 'Preact' } })

      assert_includes html, 'Preact'
      assert_includes html, 'Preact SSR'
    end
  end
end
