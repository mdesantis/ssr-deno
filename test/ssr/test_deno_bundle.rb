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

    def test_deferred_bundles_creates_and_registers
      SSR::Deno::Bundle.deferred_bundles[:deferred_test] = { path: MINIMAL_BUNDLE, auto_reload: false }

      SSR::Deno::Bundle.create_deferred_bundles!

      bundle = SSR::Deno::Bundle.registry[:deferred_test]

      refute_nil bundle
      html = bundle.render({ data: { name: 'Deferred' } })

      assert_includes html, 'Deferred'
    end

    def test_deferred_bundles_is_idempotent
      SSR::Deno::Bundle.deferred_bundles[:deferred_test] = { path: MINIMAL_BUNDLE, auto_reload: false }

      SSR::Deno::Bundle.create_deferred_bundles!
      SSR::Deno::Bundle.create_deferred_bundles!

      refute_nil SSR::Deno::Bundle.registry[:deferred_test]
    end

    def test_deferred_bundles_respects_auto_reload
      SSR::Deno::Bundle.deferred_bundles[:auto_reload_test] = { path: MINIMAL_BUNDLE, auto_reload: true }

      SSR::Deno::Bundle.create_deferred_bundles!

      bundle = SSR::Deno::Bundle.registry[:auto_reload_test]

      assert bundle.instance_variable_get(:@auto_reload)
    end

    def test_deferred_bundles_skips_already_registered
      existing = SSR::Deno::Bundle.new(MINIMAL_BUNDLE)
      SSR::Deno::Bundle.registry.register(:duplicate_test, existing)
      SSR::Deno::Bundle.deferred_bundles[:duplicate_test] = { path: MINIMAL_BUNDLE, auto_reload: false }

      SSR::Deno::Bundle.create_deferred_bundles!

      assert_same existing, SSR::Deno::Bundle.registry[:duplicate_test]
    end

    def test_create_deferred_bundles_double_check_lock_inside_mutex
      original_mutex = SSR::Deno::Bundle.instance_variable_get(:@_create_mutex)
      locked_mutex = Mutex.new
      locked_mutex.lock

      SSR::Deno::Bundle.instance_variable_set(:@_create_mutex, locked_mutex)
      SSR::Deno::Bundle.instance_variable_set(:@_deferred_created, false)

      t = Thread.new { SSR::Deno::Bundle.create_deferred_bundles! }

      start = Process.clock_gettime(Process::CLOCK_MONOTONIC)
      loop do
        break if t.status == 'sleep'
        raise 'timeout' if Process.clock_gettime(Process::CLOCK_MONOTONIC) - start > 1

        Thread.pass
      end

      SSR::Deno::Bundle.instance_variable_set(:@_deferred_created, true)
      locked_mutex.unlock

      t.join
    ensure
      SSR::Deno::Bundle.instance_variable_set(:@_create_mutex, original_mutex) if original_mutex
    end

    def teardown
      SSR::Deno::Bundle.deferred_bundles.clear
      SSR::Deno::Bundle.instance_variable_set(:@_deferred_created, false)
    end
  end
end
