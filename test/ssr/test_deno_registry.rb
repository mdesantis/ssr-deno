# frozen_string_literal: true

require 'test_helper'

module SSR
  class TestDenoRegistry < Minitest::Test
    def setup
      @registry = SSR::Deno::Bundle::Registry.new
    end

    def test_empty_registry
      assert_nil @registry[:application]
      assert_equal 0, @registry.size
    end

    def test_register_and_lookup
      bundle = Object.new
      @registry.register(:application, bundle)

      assert_same bundle, @registry[:application]
      assert_same bundle, @registry.bundle(:application)
    end

    def test_default_name_is_application
      bundle = Object.new
      @registry.register(:application, bundle)

      assert_same bundle, @registry[]
    end

    def test_register_duplicate_raises
      @registry.register(:admin, Object.new)

      assert_raises(ArgumentError) { @registry.register(:admin, Object.new) }
    end

    def test_replace_overwrites_existing
      bundle_a = Object.new
      bundle_b = Object.new
      @registry.register(:admin, bundle_a)
      @registry.replace(:admin, bundle_b)

      assert_same bundle_b, @registry[:admin]
    end

    def test_replace_creates_if_not_exists
      bundle = Object.new
      @registry.replace(:new_bundle, bundle)

      assert_same bundle, @registry[:new_bundle]
    end

    def test_remove
      bundle = Object.new
      @registry.register(:temp, bundle)
      @registry.remove(:temp)

      assert_nil @registry[:temp]
    end

    def test_each_iterates_all_bundles
      @registry.register(:application, Object.new)
      @registry.register(:admin, Object.new)

      names = []
      @registry.each { |name, _b| names << name }

      assert_includes names, :application
      assert_includes names, :admin
    end

    def test_size
      assert_equal 0, @registry.size
      @registry.register(:app, Object.new)

      assert_equal 1, @registry.size
      @registry.register(:admin, Object.new)

      assert_equal 2, @registry.size
    end

    def test_includes_enumerable
      assert_respond_to @registry, :map
      assert_respond_to @registry, :to_a
    end

    def test_thread_safety
      @registry.register(:app, Object.new)

      threads = 10.times.map do |i|
        Thread.new do
          100.times do
            @registry[:app]
          end
          @registry.register(:"thread_#{i}", Object.new)
        end
      end
      threads.each(&:join)

      assert_equal 11, @registry.size
    end
  end
end
