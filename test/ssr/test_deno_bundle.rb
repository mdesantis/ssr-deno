# frozen_string_literal: true

require 'test_helper'

module SSR
  class TestDenoBundle < Minitest::Test
    MINIMAL_BUNDLE = File.expand_path('../fixtures/minimal-bundle.js', __dir__)

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
end
