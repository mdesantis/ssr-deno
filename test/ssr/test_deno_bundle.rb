# frozen_string_literal: true

require 'test_helper'

module SSR
  class TestDenoBundle < Minitest::Test
    BUNDLE_PATH = File.expand_path('../../samples/vite-ssr-app/dist/server/entry-server.js', __dir__)
    BUNDLE = SSR::Deno::Bundle.new(BUNDLE_PATH)

    def test_render
      html = BUNDLE.render({ data: { name: 'Maurizio' } })

      assert_match(%r{<html>.*</html>}m, html)
      assert_includes html, 'Maurizio'
      assert_includes html, '<div id="root">'
    end

    def test_render_with_raw_input
      json = JSON.generate({ data: { name: 'Raw' } })
      html = BUNDLE.render(json, raw_input: true)

      assert_includes html, 'Raw'
    end

    def test_render_with_raw_output
      result = BUNDLE.render({ data: { name: 'Test' } }, raw_output: true)

      assert_instance_of String, result
      assert_includes JSON.parse(result), '<html>'
    end

    def test_render_with_raw_input_and_raw_output
      json = JSON.generate({ data: { name: 'Passthrough' } })
      result = BUNDLE.render(json, raw_input: true, raw_output: true)

      assert_instance_of String, result
      assert_includes result, 'Passthrough'
    end

    def test_multiple_bundles_coexist
      bundle_b = SSR::Deno::Bundle.new(BUNDLE_PATH)
      html_a = BUNDLE.render({ data: { name: 'Alice' } })
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
      BUNDLE.reload

      html = BUNDLE.render({ data: { name: 'Reloaded' } })

      assert_includes html, 'Reloaded'
    end

    def test_auto_reload_triggers_reload_if_changed
      BUNDLE.auto_reload = true
      html = BUNDLE.render({ data: { name: 'AutoReload' } })

      assert_includes html, 'AutoReload'
    end

    def test_reload_updates_mtime
      orig_mtime = BUNDLE.instance_variable_get(:@mtime)

      FileUtils.touch(BUNDLE_PATH)
      BUNDLE.reload

      new_mtime = BUNDLE.instance_variable_get(:@mtime)

      assert_operator new_mtime, :>, orig_mtime
    end

    def test_auto_reload_triggers_reload_when_file_changed
      BUNDLE.auto_reload = true

      FileUtils.touch(BUNDLE_PATH)

      html = BUNDLE.render({ data: { name: 'Changed' } })

      assert_includes html, 'Changed'
    end

    def test_instrument_noop_when_active_support_notifications_not_loaded
      # Temporarily undefine ActiveSupport::Notifications to exercise the
      # no-op branch of Instrumenter (core gem mode, no Rails).
      original = ActiveSupport.send(:remove_const, :Notifications)

      result = BUNDLE.send(:instrument, 'test.ssr_deno', {}) { 'yielded' }

      assert_equal 'yielded', result
    ensure
      ActiveSupport.const_set(:Notifications, original)
    end

    def test_instrument_with_active_support_notifications
      result = BUNDLE.send(:instrument, 'test.ssr_deno', {}) { 'yielded' }

      assert_equal 'yielded', result
    end
  end
end
