# frozen_string_literal: true

require 'test_helper'

module SSR
  class TestDenoBundle < Minitest::Test
    include TestFixturePaths

    def setup
      @bundle = SSR::Deno::Bundle.new(MINIMAL_BUNDLE)
    end

    def test_render
      html = @bundle.render({ data: { name: 'Maurizio' } })

      assert_includes html, 'Maurizio'
    end

    def test_render_with_raw_input
      json = JSON.generate({ data: { name: 'Raw' } })
      html = @bundle.render(json, raw_input: true)

      assert_includes html, 'Raw'
    end

    def test_render_with_raw_output
      result = @bundle.render({ data: { name: 'Test' } }, raw_output: true)

      assert_instance_of String, result
      assert_includes JSON.parse(result), 'Test'
    end

    def test_render_with_raw_input_and_raw_output
      json = JSON.generate({ data: { name: 'Passthrough' } })
      result = @bundle.render(json, raw_input: true, raw_output: true)

      assert_instance_of String, result
      assert_includes result, 'Passthrough'
    end

    def test_multiple_bundles_coexist
      bundle_b = SSR::Deno::Bundle.new(MINIMAL_BUNDLE)
      html_a = @bundle.render({ data: { name: 'Alice' } })
      html_b = bundle_b.render({ data: { name: 'Bob' } })

      assert_includes html_a, 'Alice'
      assert_includes html_b, 'Bob'
    end

    def test_bundle_not_found_raises_bundle_not_found_error
      assert_raises(SSR::Deno::BundleNotFoundError) do
        SSR::Deno.native_render('nonexistent_bundle_id', '{}')
      end
    end

    def test_reload
      @bundle.reload

      html = @bundle.render({ data: { name: 'Reloaded' } })

      assert_includes html, 'Reloaded'
    end

    def test_auto_reload_triggers_reload_if_changed
      @bundle.auto_reload = true

      html = @bundle.render({ data: { name: 'AutoReload' } })

      assert_includes html, 'AutoReload'
    end

    def test_reload_updates_mtime
      orig_mtime = @bundle.instance_variable_get(:@mtime)

      FileUtils.touch(MINIMAL_BUNDLE)
      @bundle.reload

      new_mtime = @bundle.instance_variable_get(:@mtime)

      assert_operator new_mtime, :>, orig_mtime
    end

    def test_auto_reload_triggers_reload_when_file_changed
      @bundle.auto_reload = true

      FileUtils.touch(MINIMAL_BUNDLE)

      html = @bundle.render({ data: { name: 'Changed' } })

      assert_includes html, 'Changed'
    end

    def test_instrument_noop_when_active_support_notifications_not_loaded
      original = ActiveSupport.send(:remove_const, :Notifications)
      result = @bundle.send(:instrument, 'test.ssr_deno', {}) { 'yielded' }

      assert_equal 'yielded', result
    ensure
      ActiveSupport.const_set(:Notifications, original)
    end

    def test_instrument_with_active_support_notifications
      result = @bundle.send(:instrument, 'test.ssr_deno', {}) { 'yielded' }

      assert_equal 'yielded', result
    end
  end

  class TestDenoBundleWithManifest < Minitest::Test
    include TestFixturePaths

    MANIFEST_PATH = File.join(FIXTURES_DIR, 'vite-manifest.json').freeze

    def setup
      @bundle = SSR::Deno::Bundle.new(
        MINIMAL_BUNDLE,
        manifest_path: MANIFEST_PATH,
        client_entry: 'src/entry-client.ts'
      )
    end

    def test_assets_returns_structured_hash
      result = @bundle.assets

      assert_kind_of Hash, result
      assert_includes result.keys, :css_tags
      assert_includes result.keys, :client_js_tag
      assert_includes result.keys, :asset_urls
    end

    def test_css_tags_returns_link_elements
      tags = @bundle.css_tags

      assert_includes tags, '<link rel="stylesheet" href="/assets/index-def456.css">'
      assert_includes tags, '<link rel="stylesheet" href="/assets/vendor-mno345.css">'
    end

    def test_css_tags_with_custom_prefix
      tags = @bundle.css_tags(prefix: 'https://cdn.example.com/')

      assert_includes tags, 'https://cdn.example.com/assets/'
    end

    def test_client_js_tag_returns_module_script
      tag = @bundle.client_js_tag

      assert_includes tag, '<script type="module" src="/assets/entry-client-abc123.js">'
    end

    def test_asset_urls_returns_asset_paths
      urls = @bundle.asset_urls

      assert_includes urls, '/assets/logo-ghi789.svg'
      assert_includes urls, '/assets/font-pqr678.woff2'
    end

    def test_bundle_without_manifest_returns_empty_assets
      bundle = SSR::Deno::Bundle.new(MINIMAL_BUNDLE)

      assert_equal({}, bundle.assets)
      assert_equal '', bundle.css_tags
      assert_equal '', bundle.client_js_tag
      assert_equal [], bundle.asset_urls
    end

    def test_raises_when_manifest_given_without_client_entry
      assert_raises(ArgumentError) do
        SSR::Deno::Bundle.new(MINIMAL_BUNDLE, manifest_path: MANIFEST_PATH)
      end
    end

    def test_render_still_works_with_manifest
      html = @bundle.render({ data: { name: 'WithAssets' } })

      assert_includes html, 'WithAssets'
    end
  end
end
