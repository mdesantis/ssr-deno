# frozen_string_literal: true

require 'test_helper'

module SSR
  class TestDenoManifest < Minitest::Test
    include TestFixturePaths

    MANIFEST_PATH = File.join(FIXTURES_DIR, 'vite-manifest.json').freeze

    def setup
      @manifest = SSR::Deno::Manifest.new(MANIFEST_PATH)
    end

    def test_initializes_with_valid_path
      assert_instance_of SSR::Deno::Manifest, @manifest
      assert_kind_of Hash, @manifest.data
    end

    def test_raises_on_missing_file
      assert_raises(ArgumentError) do
        SSR::Deno::Manifest.new('/nonexistent/path.json')
      end
    end

    def test_css_files_returns_css_for_entry
      css = @manifest.css_files('src/entry-client.ts')

      assert_includes css, 'assets/index-def456.css'
      assert_includes css, 'assets/vendor-mno345.css'
    end

    def test_css_files_returns_empty_for_unknown_entry
      assert_equal [], @manifest.css_files('nonexistent.ts')
    end

    def test_js_files_returns_main_and_imports
      js = @manifest.js_files('src/entry-client.ts')

      assert_equal ['assets/entry-client-abc123.js', 'assets/_vendor-jkl012.js'], js
    end

    def test_js_files_returns_empty_for_unknown_entry
      assert_equal [], @manifest.js_files('nonexistent.ts')
    end

    def test_asset_files_returns_assets_from_entry_and_imports
      assets = @manifest.asset_files('src/entry-client.ts')

      assert_includes assets, 'assets/logo-ghi789.svg'
      assert_includes assets, 'assets/font-pqr678.woff2'
    end

    def test_asset_files_returns_empty_for_unknown_entry
      assert_equal [], @manifest.asset_files('nonexistent.ts')
    end

    def test_css_tags_generates_link_elements
      tags = @manifest.css_tags('src/entry-client.ts')

      assert_includes tags, '<link rel="stylesheet" href="/assets/index-def456.css">'
      assert_includes tags, '<link rel="stylesheet" href="/assets/vendor-mno345.css">'
    end

    def test_css_tags_with_custom_prefix
      tags = @manifest.css_tags('src/entry-client.ts', prefix: 'https://cdn.example.com/')

      assert_includes tags, '<link rel="stylesheet" href="https://cdn.example.com/assets/index-def456.css">'
    end

    def test_client_js_tag_generates_module_script
      tag = @manifest.client_js_tag('src/entry-client.ts')

      assert_includes tag, '<script type="module" src="/assets/entry-client-abc123.js">'
    end

    def test_client_js_tag_returns_empty_for_unknown_entry
      assert_equal '', @manifest.client_js_tag('nonexistent.ts')
    end

    def test_all_js_tags_generates_scripts_for_all_js_files
      tags = @manifest.all_js_tags('src/entry-client.ts')

      assert_includes tags, '<script type="module" src="/assets/entry-client-abc123.js">'
      assert_includes tags, '<script type="module" src="/assets/_vendor-jkl012.js">'
    end

    def test_asset_urls_returns_prefixed_paths
      urls = @manifest.asset_urls('src/entry-client.ts')

      assert_includes urls, '/assets/logo-ghi789.svg'
      assert_includes urls, '/assets/font-pqr678.woff2'
    end

    def test_asset_urls_with_custom_prefix
      urls = @manifest.asset_urls('src/entry-client.ts', prefix: 'https://cdn.example.com/')

      assert_includes urls, 'https://cdn.example.com/assets/logo-ghi789.svg'
    end

    def test_assets_returns_structured_hash
      result = @manifest.assets('src/entry-client.ts')

      assert_kind_of Hash, result
      assert_includes result.keys, :css_tags
      assert_includes result.keys, :client_js_tag
      assert_includes result.keys, :asset_urls
      assert_includes result[:css_tags], 'stylesheet'
      assert_includes result[:client_js_tag], 'module'
      assert_kind_of Array, result[:asset_urls]
    end

    def test_assets_with_custom_prefix
      result = @manifest.assets('src/entry-client.ts', prefix: '/static/')

      assert_includes result[:css_tags], '/static/assets/'
      assert_includes result[:client_js_tag], '/static/assets/'
      assert_includes result[:asset_urls].first, '/static/assets/'
    end
  end

  class TestDenoManifestMinimal < Minitest::Test
    include TestFixturePaths

    MINIMAL_MANIFEST_PATH = File.join(FIXTURES_DIR, 'vite-manifest-minimal.json').freeze

    def setup
      @manifest = SSR::Deno::Manifest.new(MINIMAL_MANIFEST_PATH)
    end

    def test_css_files_for_entry_without_imports
      css = @manifest.css_files('src/entry-simple.ts')

      assert_equal [], css
    end

    def test_js_files_for_entry_without_imports
      js = @manifest.js_files('src/entry-simple.ts')

      assert_equal ['assets/entry-simple-xyz789.js'], js
    end

    def test_asset_files_for_entry_without_imports
      assets = @manifest.asset_files('src/entry-simple.ts')

      assert_equal [], assets
    end

    def test_css_files_for_entry_with_import_without_css
      css = @manifest.css_files('src/entry-with-imports.ts')

      assert_equal ['assets/main-def456.css'], css
    end

    def test_asset_files_for_entry_with_import_without_assets
      assets = @manifest.asset_files('src/entry-with-imports.ts')

      assert_equal [], assets
    end

    def test_client_js_tag_for_entry_without_file_returns_empty
      empty_manifest_path = File.join(FIXTURES_DIR, 'vite-manifest-empty.json')
      File.write(empty_manifest_path, '{}')
      manifest = SSR::Deno::Manifest.new(empty_manifest_path)

      tag = manifest.client_js_tag('nonexistent.ts')

      assert_equal '', tag
    ensure
      FileUtils.rm_f(empty_manifest_path)
    end
  end

  class TestDenoManifestMixed < Minitest::Test
    include TestFixturePaths

    MIXED_MANIFEST_PATH = File.join(FIXTURES_DIR, 'vite-manifest-mixed.json').freeze

    def setup
      @manifest = SSR::Deno::Manifest.new(MIXED_MANIFEST_PATH)
    end

    def test_css_files_with_mixed_imports
      css = @manifest.css_files('src/entry-mixed.ts')

      assert_includes css, 'assets/main.css'
      assert_includes css, 'assets/with-css.css'
      refute_includes css, 'assets/no-css.css'
    end

    def test_asset_files_with_mixed_imports
      assets = @manifest.asset_files('src/entry-mixed.ts')

      assert_equal ['assets/logo.svg'], assets
    end

    def test_js_files_with_mixed_imports
      js = @manifest.js_files('src/entry-mixed.ts')

      assert_equal ['assets/entry-mixed-abc.js', 'assets/with-css.js', 'assets/no-css.js'], js
    end

    def test_css_files_with_missing_import
      css = @manifest.css_files('src/entry-bad-import.ts')

      assert_equal [], css
    end

    def test_js_files_with_import_without_file
      js = @manifest.js_files('src/entry-mixed.ts')

      assert_equal ['assets/entry-mixed-abc.js', 'assets/with-css.js', 'assets/no-css.js'], js
      refute_includes js, nil
    end

    def test_asset_files_with_import_without_file
      assets = @manifest.asset_files('src/entry-mixed.ts')

      assert_equal ['assets/logo.svg'], assets
    end

    def test_asset_files_with_missing_import
      assets = @manifest.asset_files('src/entry-bad-asset-import.ts')

      assert_equal ['assets/real.svg'], assets
    end
  end
end
