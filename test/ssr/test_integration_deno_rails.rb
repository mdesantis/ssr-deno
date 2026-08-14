# frozen_string_literal: true

# Integration tests for the Rails integration (Railtie, Helper, Generator).
# Runs via Combustion (in-memory Rails app booted by test_helper_rails.rb).
#
# Run with:
#   bundle exec rake test:rails

require_relative '../support/fixture_paths'

module SSR
  module Deno
    class TestRailsIntegration < ::Minitest::Test
      include TestFixturePaths

      def setup
        @view = build_view
      end

      def teardown
        SSR::Deno::Bundle.registry.clear
        SSR::Deno::Bundle.instance_variable_set(:@_bundles_created, false)
      end

      def test_railtie_registers_config
        assert_respond_to Rails.application.config, :ssr_deno,
                          'Railtie should register config.ssr_deno'

        assert Rails.application.config.ssr_deno.enabled
        # raise_on_render_error defaults to !Rails.env.production?, which is true in test
        assert Rails.application.config.ssr_deno.raise_on_render_error
        # node_builtins_enabled defaults to nil (which means false)
        assert_nil Rails.application.config.ssr_deno.node_builtins_enabled
        assert Rails.application.config.ssr_deno.raise_on_bundle_error
      end

      def test_railtie_sets_default_bundles
        bundles = Rails.application.config.ssr_deno.bundles

        assert_empty bundles,
                     'Bundles should be empty by default'
      end

      def test_helper_included_in_action_view
        assert_includes ActionView::Base.ancestors, SSR::Deno::RailsHelper,
                        'Helper should be included in ActionView::Base'
      end

      def test_registry_accessible
        assert_instance_of Hash,
                           SSR::Deno::Bundle.registry
      end

      def test_registry_empty_by_default
        assert_equal 0, SSR::Deno::Bundle.registry.size
      end

      def test_subscribe_heap_stats_disabled_for_non_positive_rate
        assert_nil SSR::Deno::Railtie.subscribe_heap_stats(0)
        assert_nil SSR::Deno::Railtie.subscribe_heap_stats(-1)
        assert_nil SSR::Deno::Railtie.subscribe_heap_stats(nil)
      end

      def test_subscribe_heap_stats_emits_every_nth_render
        SSR::Deno::Bundle.registry[:application] = {
          path: MINIMAL_BUNDLE, auto_reload: false
        }

        subscriber = SSR::Deno::Railtie.subscribe_heap_stats(2)

        refute_nil subscriber

        events = []
        callback = ->(_name, _start, _finish, _id, payload) { events << payload }

        begin
          ActiveSupport::Notifications.subscribed(callback, 'heap_stats.ssr_deno') do
            2.times { @view.ssr_render({ data: { name: 'Heap' } }) }
          end
        ensure
          ActiveSupport::Notifications.unsubscribe(subscriber)
        end

        assert_equal 1, events.size, 'rate 2 must emit once per two renders'
        assert_operator events.first.size, :>, 0
      end

      def test_subscribe_heap_stats_logs_collection_failure
        subscriber = SSR::Deno::Railtie.subscribe_heap_stats(1)
        original_logger = Rails.logger
        log_output = StringIO.new
        klass = SSR::Deno::HeapStats.singleton_class

        Rails.logger = ActiveSupport::Logger.new(log_output)
        klass.alias_method(:original_fetch, :fetch)
        klass.define_method(:fetch) { raise SSR::Deno::Error, 'boom' }

        begin
          ActiveSupport::Notifications.instrument('render.ssr_deno', bundle_name: 'x')
        ensure
          klass.alias_method(:fetch, :original_fetch)
          klass.remove_method(:original_fetch)

          Rails.logger = original_logger

          ActiveSupport::Notifications.unsubscribe(subscriber)
        end

        assert_includes log_output.string, 'Failed to collect heap stats: boom'
      end

      def test_ssr_render_returns_empty_string_when_disabled
        Rails.application.config.ssr_deno.enabled = false

        assert_equal '', @view.ssr_render({ data: { name: 'Disabled' } })
      ensure
        Rails.application.config.ssr_deno.enabled = true
      end

      def test_ssr_render_raises_on_unknown_option
        error = assert_raises ArgumentError do
          @view.ssr_render({}, raw_ouputput: true)
        end

        assert_match(/raw_ouputput/, error.message)
      end

      def test_ssr_render_raises_bundle_not_found
        error = assert_raises SSR::Deno::BundleNotFoundError do
          @view.ssr_render({ page: 'home' })
        end

        assert_match(/not registered/, error.message)
      end

      def test_ssr_render_with_nonexistent_bundle_name
        error = assert_raises SSR::Deno::BundleNotFoundError do
          @view.ssr_render({}, bundle: :nonexistent)
        end

        assert_match(/nonexistent/, error.message)
      end

      def test_instrumentation_fires_bundle_miss_event
        events = []
        callback = ->(name, _start, _finish, _id, payload) { events << [name, payload] }

        ActiveSupport::Notifications.subscribed(callback, /\.ssr_deno$/) do
          error = assert_raises SSR::Deno::BundleNotFoundError do
            @view.ssr_render({ page: 'home' })
          end

          assert_match(/not registered/, error.message)
        end

        event_names = events.map(&:first)

        assert_includes event_names, 'bundle_miss.ssr_deno',
                        'bundle_miss.ssr_deno event should fire when bundle not found'
        assert_includes event_names, 'ssr_render.ssr_deno',
                        'ssr_render.ssr_deno event should fire on render'

        render_event = events.assoc('ssr_render.ssr_deno')

        assert render_event, 'ssr_render.ssr_deno event should be present'
        assert_equal :application, render_event.last[:bundle_name]
        assert_match(/not registered/, render_event.last[:error].to_s)
      end

      def test_ssr_render_happy_path
        SSR::Deno::Bundle.registry[:application] = {
          path: MINIMAL_BUNDLE, auto_reload: false
        }

        html = @view.ssr_render({ data: { name: 'Rails' } })

        assert_includes html, 'Rails'
      end

      def test_ssr_render_csr_fallback_on_bundle_not_found
        Rails.application.config.ssr_deno.raise_on_bundle_error = false

        result = @view.ssr_render({ page: 'home' })

        assert_equal '', result
      ensure
        Rails.application.config.ssr_deno.raise_on_bundle_error = true
      end

      def test_ssr_render_csr_fallback_on_render_error
        Rails.application.config.ssr_deno.raise_on_render_error = false

        SSR::Deno::Bundle.registry[:application] = {
          path: MINIMAL_BUNDLE, auto_reload: false
        }
        SSR::Deno::Bundle.create_bundles!

        bundle = SSR::Deno::Bundle.registry[:application]
        def bundle.render(...)
          raise SSR::Deno::RenderError, 'simulated'
        end

        result = @view.ssr_render({})

        assert_equal '', result
      ensure
        Rails.application.config.ssr_deno.raise_on_render_error = true
      end

      def test_railtie_config_nil_defaults_do_not_override_native_defaults
        assert_nil Rails.application.config.ssr_deno.max_heap_size_mb
        assert_equal 64, SSR::Deno::Config.max_heap_size_mb

        assert_nil Rails.application.config.ssr_deno.isolate_pool_size
        assert_equal 1, SSR::Deno::Config.isolate_pool_size

        assert_nil Rails.application.config.ssr_deno.render_timeout_ms
        assert_equal 500, SSR::Deno::Config.render_timeout_ms
      end

      def test_render_chunks_instrumentation_fires_render_event
        SSR::Deno::Bundle.registry[:application] = {
          path: MINIMAL_BUNDLE, auto_reload: false
        }

        events = []
        callback = ->(name, _start, _finish, _id, payload) { events << [name, payload] }

        ActiveSupport::Notifications.subscribed(callback, /render\.ssr_deno/) do
          @view.ssr_render({ data: { name: 'Rails' } })
        end

        event = events.assoc('render.ssr_deno')

        assert event, 'render.ssr_deno event should fire on render'
        assert_equal MINIMAL_BUNDLE, event.last[:bundle_name]
        assert_nil event.last[:error]
      end

      def test_render_chunks_instrumentation_fires_event
        bundle = SSR::Deno::Bundle.new(MINIMAL_BUNDLE)

        events = []
        callback = ->(name, _start, _finish, _id, payload) { events << [name, payload] }

        ActiveSupport::Notifications.subscribed(callback, /render\.ssr_deno/) do
          bundle.render_chunks({ data: { name: 'chunked-test' } }) { |_| nil }
        end

        event = events.assoc('render.ssr_deno')

        assert event, 'render.ssr_deno event should fire on render_chunks'
        assert_equal MINIMAL_BUNDLE, event.last[:bundle_name]
        assert_nil event.last[:error]
      end

      def test_render_chunks_instrumentation_records_error
        bundle = SSR::Deno::Bundle.new(REJECT_BUNDLE)

        events = []
        callback = ->(name, _start, _finish, _id, payload) { events << [name, payload] }

        ActiveSupport::Notifications.subscribed(callback, /render\.ssr_deno/) do
          assert_raises(SSR::Deno::RenderError) do
            bundle.render_chunks({}) { |_| nil }
          end
        end

        event = events.assoc('render.ssr_deno')

        assert event, 'render.ssr_deno event should fire even when the render fails'
        refute_nil event.last[:error], 'failed chunked render must record :error in the payload'
      end

      private

      def build_view
        lookup = ActionView::LookupContext.new([])

        ActionView::Base.new(lookup, {}, nil)
      end
    end
  end
end
