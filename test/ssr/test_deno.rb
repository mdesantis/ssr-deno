# frozen_string_literal: true

require 'test_helper'

module SSR
  class TestDeno < Minitest::Test
    MINIMAL_BUNDLE = File.expand_path('../fixtures/minimal-bundle.js', __dir__)

    def test_that_it_has_a_version_number
      refute_nil ::SSR::Deno::VERSION
    end

    def test_native_version
      assert_match(/\A\d+\.\d+\.\d+/, ::SSR::Deno.native_version)
    end

    def test_set_max_heap_size_mb
      SSR::Deno.max_heap_size_mb = 128
    rescue SSR::Deno::JsRuntimeInitializationError
      # pool already initialized, that's fine
    end

    def test_set_isolate_pool_size
      SSR::Deno.isolate_pool_size = 4
    rescue SSR::Deno::JsRuntimeInitializationError
      # pool already initialized, that's fine
    end

    def test_set_render_timeout_ms
      SSR::Deno.render_timeout_ms = 200
    rescue SSR::Deno::JsRuntimeInitializationError
      # pool already initialized, that's fine
    end

    def test_heap_stats
      SSR::Deno::Bundle.new(MINIMAL_BUNDLE)

      stats = SSR::Deno.heap_stats

      assert_kind_of Hash, stats
      assert stats.key?('total_heap_size')
      assert stats.key?('used_heap_size')
      assert stats.key?('heap_size_limit')
      assert_kind_of Integer, stats['total_heap_size']
      assert_operator stats['total_heap_size'], :>, 0
    end

    def test_heap_stats_bang_raises_when_uninitialized
      klass = SSR::Deno.singleton_class
      klass.alias_method(:original_native_heap_stats, :native_heap_stats)
      klass.define_method(:native_heap_stats) do
        raise SSR::Deno::JsRuntimeNotInitializedError, 'pool not initialized'
      end

      assert_raises(SSR::Deno::JsRuntimeNotInitializedError) do
        SSR::Deno.heap_stats!
      end
    ensure
      klass.alias_method(:native_heap_stats, :original_native_heap_stats)
      klass.remove_method(:original_native_heap_stats)
    end

    def test_heap_stats_before_initialization_returns_empty_hash
      klass = SSR::Deno.singleton_class

      klass.alias_method(:original_native_heap_stats, :native_heap_stats)
      klass.define_method(:native_heap_stats) do
        raise SSR::Deno::JsRuntimeNotInitializedError, 'pool not initialized'
      end

      assert_output(nil, /not initialized/) do
        stats = SSR::Deno.heap_stats

        assert_equal({}, stats)
      end
    ensure
      klass.alias_method(:native_heap_stats, :original_native_heap_stats)
      klass.remove_method(:original_native_heap_stats)
    end
  end
end
