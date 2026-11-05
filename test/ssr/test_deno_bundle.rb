# frozen_string_literal: true

require 'test_helper'

module SSR
  class TestDenoBundle < Minitest::Test
    include TestFixturePaths
    prepend TempBundleHelper

    def setup
      @bundle = SSR::Deno::Bundle.new(MINIMAL_BUNDLE)
      SSR::Deno::Bundle.instance_variable_set(:@_bundles_created, false)
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

    def test_auto_reload_picks_up_new_file_content
      bundle_path = File.join(temp_dir, 'hmr-bundle.js')
      File.write(bundle_path, <<~JS)
        globalThis.render = function() { return '<h1>v1</h1>'; };
      JS

      bundle = SSR::Deno::Bundle.new(bundle_path)
      bundle.auto_reload = true

      assert_includes bundle.render({}), 'v1'

      File.write(bundle_path, <<~JS)
        globalThis.render = function() { return '<h1>v2</h1>'; };
      JS

      assert_includes bundle.render({}), 'v2'

      # Third render — no file change, exercises "unchanged" branch of reload_if_changed
      assert_includes bundle.render({}), 'v2'
    end

    def test_instrument_noop_when_active_support_notifications_not_loaded
      original = ActiveSupport.send(:remove_const, :Notifications)
      result = @bundle.send(:instrument, 'test.ssr_deno', {}) { 'yielded' }

      assert_equal 'yielded', result
    ensure
      ActiveSupport.const_set(:Notifications, original)
    end

    def test_instrument_noop_without_block_does_not_raise
      original = ActiveSupport.send(:remove_const, :Notifications)

      assert_nil @bundle.send(:instrument, 'test.ssr_deno', {})
    ensure
      ActiveSupport.const_set(:Notifications, original)
    end

    def test_instrument_with_active_support_notifications
      result = @bundle.send(:instrument, 'test.ssr_deno', {}) { 'yielded' }

      assert_equal 'yielded', result
    end

    def test_registry_creates_bundles
      SSR::Deno::Bundle.registry[:test] = { path: MINIMAL_BUNDLE, auto_reload: false }

      SSR::Deno::Bundle.create_bundles!

      bundle = SSR::Deno::Bundle.registry[:test]

      assert_instance_of SSR::Deno::Bundle, bundle
      html = bundle.render({ data: { name: 'Created' } })

      assert_includes html, 'Created'
    end

    def test_create_bundles_is_idempotent
      SSR::Deno::Bundle.registry[:idempotent_test] = { path: MINIMAL_BUNDLE, auto_reload: false }

      SSR::Deno::Bundle.create_bundles!
      SSR::Deno::Bundle.create_bundles!

      assert_instance_of SSR::Deno::Bundle, SSR::Deno::Bundle.registry[:idempotent_test]
    end

    def test_create_bundles_respects_auto_reload
      SSR::Deno::Bundle.registry[:auto_reload_test] = { path: MINIMAL_BUNDLE, auto_reload: true }

      SSR::Deno::Bundle.create_bundles!

      bundle = SSR::Deno::Bundle.registry[:auto_reload_test]

      assert bundle.instance_variable_get(:@auto_reload)
    end

    def test_create_bundles_skips_missing_file
      missing = File.join(temp_dir, 'entry-server.js')

      SSR::Deno::Bundle.registry[:missing] = { path: missing, auto_reload: false }
      SSR::Deno::Bundle.registry[:present] = { path: MINIMAL_BUNDLE, auto_reload: false }

      SSR::Deno::Bundle.create_bundles!

      assert_equal({ path: missing, auto_reload: false }, SSR::Deno::Bundle.registry[:missing])
      assert_instance_of SSR::Deno::Bundle, SSR::Deno::Bundle.registry[:present]
    end

    def test_create_bundles_tolerates_registration_mid_walk
      entered = Queue.new
      release = Queue.new
      blocking_path = Object.new

      # File.exist? calls #to_path, so this parks the create_bundles! walk
      # mid-iteration — deterministic overlap, no sleeps.
      blocking_path.define_singleton_method(:to_path) do
        entered.push(true)
        release.pop

        File.join(Dir.tmpdir, "ssr-deno-missing-#{Process.pid}.js")
      end

      SSR::Deno::Bundle.registry[:blocking] = { path: blocking_path, auto_reload: false }

      walker = Thread.new { SSR::Deno::Bundle.create_bundles! }

      entered.pop

      # A DevModeBundle registering itself from another thread does exactly
      # this write, without the mutex.
      SSR::Deno::Bundle.registry[:added_mid_walk] = Object.new

      assert SSR::Deno::Bundle.registry.key?(:added_mid_walk)
    ensure
      release&.push(true)
      walker&.join
    end

    def test_fetch_returns_nil_for_unregistered_name
      assert_nil SSR::Deno::Bundle.fetch(:unregistered)
    end

    def test_fetch_returns_instantiated_bundle
      SSR::Deno::Bundle.registry[:fetched] = { path: MINIMAL_BUNDLE, auto_reload: false }

      bundle = SSR::Deno::Bundle.fetch(:fetched)

      assert_instance_of SSR::Deno::Bundle, bundle
      assert_same bundle, SSR::Deno::Bundle.fetch(:fetched)
    end

    def test_fetch_raises_while_file_missing
      missing = File.join(temp_dir, 'entry-server.js')

      SSR::Deno::Bundle.registry[:late] = { path: missing, auto_reload: false }

      error = assert_raises(SSR::Deno::BundleNotFoundError) do
        SSR::Deno::Bundle.fetch(:late)
      end

      assert_match(/:late not found at #{Regexp.escape(missing)}/, error.message)
    end

    def test_fetch_instantiates_bundle_built_after_create_bundles
      missing = File.join(temp_dir, 'entry-server.js')

      SSR::Deno::Bundle.registry[:late] = { path: missing, auto_reload: true }

      SSR::Deno::Bundle.create_bundles!

      FileUtils.cp(MINIMAL_BUNDLE, missing)

      bundle = SSR::Deno::Bundle.fetch(:late)

      assert_instance_of SSR::Deno::Bundle, bundle
      assert bundle.auto_reload
      assert_includes bundle.render({ data: { name: 'Late' } }), 'Late'
    end

    def test_fetch_rechecks_entry_under_mutex
      SSR::Deno::Bundle.registry[:raced] = { path: MINIMAL_BUNDLE, auto_reload: false }

      SSR::Deno::Bundle.create_bundles!

      winner = SSR::Deno::Bundle.registry[:raced]

      # Simulate another thread materializing the entry between the
      # unsynchronized read and the mutex: put a config hash back, then have
      # the mutex swap in the winner before the re-check runs.
      SSR::Deno::Bundle.registry[:raced] = { path: MINIMAL_BUNDLE, auto_reload: false }

      mutex = SSR::Deno::Bundle.instance_variable_get(:@_create_mutex)

      mutex.define_singleton_method(:synchronize) do |&block|
        SSR::Deno::Bundle.registry[:raced] = winner

        block.call
      end

      assert_same winner, SSR::Deno::Bundle.fetch(:raced)
    ensure
      mutex&.singleton_class&.remove_method(:synchronize)
    end

    def test_create_bundles_outer_guard
      original_mutex = SSR::Deno::Bundle.instance_variable_get(:@_create_mutex)
      locked_mutex = Mutex.new
      blocking = Queue.new

      locked_mutex.lock

      SSR::Deno::Bundle.instance_variable_set(:@_create_mutex, locked_mutex)
      SSR::Deno::Bundle.instance_variable_set(:@_bundles_created, false)

      t = Thread.new do
        # Signal we're about to try locking
        blocking.push(true)

        SSR::Deno::Bundle.create_bundles!
      end

      # Wait for thread to reach the lock attempt
      blocking.pop

      # Give thread time to block on mutex
      sleep 0.01

      SSR::Deno::Bundle.instance_variable_set(:@_bundles_created, true)
      locked_mutex.unlock

      t.join
    ensure
      begin
        locked_mutex.unlock if locked_mutex&.locked?
      rescue StandardError
        nil
      end
      SSR::Deno::Bundle.instance_variable_set(:@_create_mutex, original_mutex) if original_mutex
    end

    def teardown
      SSR::Deno::Bundle.registry.clear
      SSR::Deno::Bundle.instance_variable_set(:@_bundles_created, false)
    end
  end
end
