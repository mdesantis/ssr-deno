# frozen_string_literal: true

# Integration tests for the Rails dummy app.
# These tests boot a real Rails application and verify that the Railtie,
# Helper, and Generator work correctly together.
#
# Run with:
#   BUNDLE_GEMFILE=test/dummy/Gemfile bundle exec ruby -I test/dummy -I lib -I test -e '
#     require "test_helper"
#     require "ssr/deno/rails"
#     require "test/ssr/test_deno_rails_integration"
#   '

module SSR
  module Deno
    class TestRailsIntegration < ::Minitest::Test
      def setup
        # Ensure we're in a Rails context
        skip 'Rails dummy app not available' unless defined?(Rails) && Rails.application

        @view = build_view
      end

      def test_railtie_registers_config
        assert_respond_to Rails.application.config, :ssr_deno,
                          'Railtie should register config.ssr_deno'

        assert Rails.application.config.ssr_deno.enabled
        # raise_on_render_error defaults to !Rails.env.production?, which is true in test
        assert Rails.application.config.ssr_deno.raise_on_render_error
      end

      def test_railtie_sets_default_bundles
        bundles = Rails.application.config.ssr_deno.bundles

        assert bundles.key?(:application),
               'Default bundles should include :application'
      end

      def test_helper_included_in_action_view
        assert_includes ActionView::Base.ancestors, SSR::Deno::Helper,
                        'Helper should be included in ActionView::Base'
      end

      def test_registry_accessible
        assert_instance_of SSR::Deno::Bundle::Registry,
                           SSR::Deno::Bundle.registry
      end

      def test_registry_empty_by_default
        # No bundle files exist in the dummy app, so none should be registered
        assert_equal 0, SSR::Deno::Bundle.registry.size
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

      private

      def build_view
        lookup = ActionView::LookupContext.new([])
        ActionView::Base.new(lookup, {}, nil)
      end
    end
  end
end
