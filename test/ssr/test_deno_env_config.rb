# frozen_string_literal: true

require 'support/subprocess_helper'

module SSR
  class TestDenoEnvConfig < Minitest::Test
    include SubprocessHelper

    def refute_subprocess(script, msg, env: {})
      _, _, status = run_subprocess(script, env: env)

      refute_predicate status, :success?, msg
    end

    def test_max_heap_size_mb_from_env
      script = 'exit 0 if SSR::Deno.max_heap_size_mb == 128; exit 1'

      assert_subprocess(script,
                        'Expected max_heap_size_mb=128 from env',
                        env: { 'SSR_DENO_MAX_HEAP_SIZE_MB' => '128' })
    end

    def test_isolate_pool_size_from_env
      script = 'exit 0 if SSR::Deno.isolate_pool_size == 4; exit 1'

      assert_subprocess(script,
                        'Expected isolate_pool_size=4 from env',
                        env: { 'SSR_DENO_ISOLATE_POOL_SIZE' => '4' })
    end

    def test_render_timeout_ms_from_env
      script = 'exit 0 if SSR::Deno.render_timeout_ms == 1000; exit 1'

      assert_subprocess(script,
                        'Expected render_timeout_ms=1000 from env',
                        env: { 'SSR_DENO_RENDER_TIMEOUT_MS' => '1000' })
    end

    def test_node_builtins_enabled_from_env_true
      script = 'exit 0 if SSR::Deno.node_builtins_enabled?; exit 1'

      assert_subprocess(script,
                        'Expected node_builtins_enabled?=true from env',
                        env: { 'SSR_DENO_NODE_BUILTINS_ENABLED' => 'true' })
    end

    def test_node_builtins_enabled_from_env_false
      script = 'exit 0 unless SSR::Deno.node_builtins_enabled?; exit 1'

      assert_subprocess(script,
                        'Expected node_builtins_enabled?=false from env',
                        env: { 'SSR_DENO_NODE_BUILTINS_ENABLED' => 'false' })
    end

    def test_setter_overrides_env_var
      script = <<~RUBY
        SSR::Deno.max_heap_size_mb = 256
        exit 0 if SSR::Deno.max_heap_size_mb == 256; exit 1
      RUBY

      assert_subprocess(script,
                        'Expected setter to override env var',
                        env: { 'SSR_DENO_MAX_HEAP_SIZE_MB' => '64' })
    end

    def test_boolean_true_variants
      %w[true 1 yes TRUE True Yes].each do |val|
        script = 'exit 0 if SSR::Deno.node_builtins_enabled?; exit 1'

        assert_subprocess(script,
                          "Expected true for #{val}",
                          env: { 'SSR_DENO_NODE_BUILTINS_ENABLED' => val })
      end
    end

    def test_boolean_false_variants
      %w[false 0 no False No].each do |val|
        script = 'exit 0 unless SSR::Deno.node_builtins_enabled?; exit 1'

        assert_subprocess(script,
                          "Expected false for #{val}",
                          env: { 'SSR_DENO_NODE_BUILTINS_ENABLED' => val })
      end
    end

    def test_boolean_empty_string_is_false
      script = 'exit 0 unless SSR::Deno.node_builtins_enabled?; exit 1'

      assert_subprocess(script,
                        'Expected false for empty string',
                        env: { 'SSR_DENO_NODE_BUILTINS_ENABLED' => '' })
    end

    def test_boolean_unrecognised_value_warns
      _, stderr, status = run_subprocess(
        'exit 0 unless SSR::Deno.node_builtins_enabled?; exit 1',
        env: { 'SSR_DENO_NODE_BUILTINS_ENABLED' => 'treu' }
      )

      assert_predicate status, :success?,
                       'Expected false for unrecognised boolean'
      assert_includes stderr, 'Unrecognised boolean'
    end

    def test_invalid_integer_format_warns_and_skips
      script = 'exit 0 if SSR::Deno.max_heap_size_mb == 64; exit 1'

      assert_subprocess(script,
                        'Expected default 64 when env var is invalid',
                        env: { 'SSR_DENO_MAX_HEAP_SIZE_MB' => 'abc' })
    end

    def test_env_var_not_set_uses_native_default
      env = {
        'SSR_DENO_MAX_HEAP_SIZE_MB' => nil,
        'SSR_DENO_ISOLATE_POOL_SIZE' => nil,
        'SSR_DENO_RENDER_TIMEOUT_MS' => nil,
        'SSR_DENO_NODE_BUILTINS_ENABLED' => nil
      }
      script = 'exit 0 if SSR::Deno.max_heap_size_mb == 64; exit 1'

      assert_subprocess(script,
                        'Expected default 64 when env var not set',
                        env: env)
    end

    def test_env_var_used_in_pool_init
      script = <<~RUBY
        bundle_path = File.join('#{TestFixturePaths::GEM_ROOT}', 'test', 'fixtures', 'minimal-bundle.js')
        bundle = SSR::Deno::Bundle.new(bundle_path)
        stats = SSR::Deno.heap_stats
        exit 0 if stats.is_a?(Hash) && !stats.empty?; exit 1
      RUBY

      assert_subprocess(script,
                        'Expected pool to use 128 MB from env',
                        env: { 'SSR_DENO_MAX_HEAP_SIZE_MB' => '128' })
    end

    def test_env_var_set_but_pool_already_initialized_raises
      script = <<~RUBY
        bundle_path = File.join('#{TestFixturePaths::GEM_ROOT}', 'test', 'fixtures', 'minimal-bundle.js')
        bundle = SSR::Deno::Bundle.new(bundle_path)
        begin
          SSR::Deno.max_heap_size_mb = 256
          exit 1
        rescue SSR::Deno::JsRuntimeInitializationError
          exit 0
        end
      RUBY

      assert_subprocess(script,
                        'Expected JsRuntimeInitializationError',
                        env: { 'SSR_DENO_MAX_HEAP_SIZE_MB' => '128' })
    end
  end
end
