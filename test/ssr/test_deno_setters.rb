# frozen_string_literal: true

require 'open3'
require 'rbconfig'

module SSR
  class TestDenoSetters < Minitest::Test
    GEM_ROOT = File.expand_path('../..', __dir__)

    BOOTSTRAP = <<~RUBY
      $LOAD_PATH.unshift('lib')
      require 'ssr/deno'
    RUBY

    def assert_subprocess(body, msg)
      script = "#{BOOTSTRAP}#{body}"
      _, _, status = Open3.capture3(RbConfig.ruby, '-e', script, chdir: GEM_ROOT)

      assert_predicate status, :success?, msg
    end

    def test_max_heap_size_mb_before_init
      assert_subprocess(<<~RUBY, 'Expected max_heap_size_mb= to succeed before init')
        SSR::Deno.max_heap_size_mb = 128
        exit 0
      RUBY
    end

    def test_isolate_pool_size_before_init
      assert_subprocess(<<~RUBY, 'Expected isolate_pool_size= to succeed before init')
        SSR::Deno.isolate_pool_size = 2
        exit 0
      RUBY
    end

    def test_render_timeout_ms_before_init
      assert_subprocess(<<~RUBY, 'Expected render_timeout_ms= to succeed before init')
        SSR::Deno.render_timeout_ms = 500
        exit 0
      RUBY
    end

    def test_setters_raise_after_init
      assert_subprocess(<<~RUBY, 'Expected JsRuntimeInitializationError after init')
        SSR::Deno.render_timeout_ms = 100
        SSR::Deno.isolate_pool_size = 1
        bundle_path = File.join('#{GEM_ROOT}', 'test', 'fixtures', 'minimal-bundle.js')
        bundle = SSR::Deno::Bundle.new(bundle_path)
        begin
          SSR::Deno.max_heap_size_mb = 256
          exit 1
        rescue SSR::Deno::JsRuntimeInitializationError
          exit 0
        end
      RUBY
    end

    # Getter methods (coverage for native getter delegation)
    def test_getter_methods_are_callable
      assert_kind_of Integer, SSR::Deno.max_heap_size_mb
      assert_kind_of Integer, SSR::Deno.isolate_pool_size
      assert_kind_of Integer, SSR::Deno.render_timeout_ms
      assert_includes [true, false], SSR::Deno.node_builtins_enabled?
    end

    # Env var apply methods (coverage for apply_integer_env and apply_bool_env)
    def test_env_var_apply_methods
      # Set env vars so the methods don't return early
      ENV['SSR_DENO_MAX_HEAP_SIZE_MB'] = '128'
      ENV['SSR_DENO_NODE_BUILTINS_ENABLED'] = 'true'

      SSR::Deno.send(:apply_integer_env, 'SSR_DENO_MAX_HEAP_SIZE_MB', :max_heap_size_mb=)
      SSR::Deno.send(:apply_bool_env, 'SSR_DENO_NODE_BUILTINS_ENABLED', :node_builtins_enabled=)

      # Also test the warning branch (invalid integer)
      ENV['SSR_DENO_MAX_HEAP_SIZE_MB'] = 'abc'
      SSR::Deno.send(:apply_integer_env, 'SSR_DENO_MAX_HEAP_SIZE_MB', :max_heap_size_mb=)
      ENV.delete('SSR_DENO_MAX_HEAP_SIZE_MB')

      # Clean up
      ENV.delete('SSR_DENO_NODE_BUILTINS_ENABLED')
    end
  end
end
