# frozen_string_literal: true

require 'support/subprocess_helper'

module SSR
  class TestDenoConfig < Minitest::Test
    include SubprocessHelper

    def test_max_heap_size_mb_before_init
      assert_subprocess(<<~RUBY, 'Expected max_heap_size_mb= to succeed before init')
        SSR::Deno::Config.max_heap_size_mb = 128
        exit 0
      RUBY
    end

    def test_isolate_pool_size_before_init
      assert_subprocess(<<~RUBY, 'Expected isolate_pool_size= to succeed before init')
        SSR::Deno::Config.isolate_pool_size = 2
        exit 0
      RUBY
    end

    def test_render_timeout_ms_before_init
      assert_subprocess(<<~RUBY, 'Expected render_timeout_ms= to succeed before init')
        SSR::Deno::Config.render_timeout_ms = 500
        exit 0
      RUBY
    end

    def test_setters_raise_after_init
      assert_subprocess(<<~RUBY, 'Expected JsRuntimeInitializationError after init')
        SSR::Deno::Config.render_timeout_ms = 100
        SSR::Deno::Config.isolate_pool_size = 1
        bundle_path = File.join('#{TestFixturePaths::GEM_ROOT}', 'test', 'fixtures', 'minimal-bundle.js')
        bundle = SSR::Deno::Bundle.new(bundle_path)
        begin
          SSR::Deno::Config.max_heap_size_mb = 256
          exit 1
        rescue SSR::Deno::JsRuntimeInitializationError
          exit 0
        end
      RUBY
    end

    def test_getter_methods_are_callable
      assert_kind_of Integer, SSR::Deno::Config.max_heap_size_mb
      assert_kind_of Integer, SSR::Deno::Config.isolate_pool_size
      assert_kind_of Integer, SSR::Deno::Config.render_timeout_ms
      assert_includes [true, false], SSR::Deno::Config.node_builtins_enabled?
    end

    def test_env_var_apply_methods
      ENV['SSR_DENO_MAX_HEAP_SIZE_MB'] = '128'
      ENV['SSR_DENO_NODE_BUILTINS_ENABLED'] = 'true'

      SSR::Deno::Config.send(:apply_integer_env, 'SSR_DENO_MAX_HEAP_SIZE_MB', :max_heap_size_mb=)

      assert_equal 128, SSR::Deno::Config.max_heap_size_mb

      SSR::Deno::Config.send(:apply_bool_env, 'SSR_DENO_NODE_BUILTINS_ENABLED', :node_builtins_enabled=)

      assert_predicate SSR::Deno::Config, :node_builtins_enabled?

      _, err = capture_io do
        ENV['SSR_DENO_MAX_HEAP_SIZE_MB'] = 'abc'
        SSR::Deno::Config.send(:apply_integer_env, 'SSR_DENO_MAX_HEAP_SIZE_MB', :max_heap_size_mb=)
      end

      assert_equal 128, SSR::Deno::Config.max_heap_size_mb
      assert_includes err, 'Cannot apply'

      _, err = capture_io do
        ENV['SSR_DENO_NODE_BUILTINS_ENABLED'] = 'treu'
        SSR::Deno::Config.send(:apply_bool_env, 'SSR_DENO_NODE_BUILTINS_ENABLED', :node_builtins_enabled=)
      end

      assert_includes err, 'Unrecognized boolean'

      ENV.delete('SSR_DENO_MAX_HEAP_SIZE_MB')
      ENV.delete('SSR_DENO_NODE_BUILTINS_ENABLED')
    end

    def test_bool_predicate_reader
      SSR::Deno::Config.node_builtins_enabled = true

      assert_predicate SSR::Deno::Config, :node_builtins_enabled?

      SSR::Deno::Config.node_builtins_enabled = false

      refute_predicate SSR::Deno::Config, :node_builtins_enabled?
    end
  end
end
