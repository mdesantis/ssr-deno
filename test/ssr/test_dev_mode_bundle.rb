# frozen_string_literal: true

require_relative '../test_helper'

module SSR
  module Deno
    class TestDevModeBundle < Minitest::Test
      ROOT = File.expand_path('../..', __dir__)
      FIXTURE = File.join(ROOT, 'test', 'fixtures', 'dev-entry.tsx')

      def test_dev_bundle_renders_entry
        bundle = DevModeBundle.new(FIXTURE)

        result = bundle.render({ input: 42 })

        assert_equal({ 'hello' => 'world', 'input' => 42 }, result)
      end

      def test_dev_bundle_registers_in_bundle_registry
        name = "test_bundle_#{Time.now.to_i}"
        bundle = DevModeBundle.new(FIXTURE, name: name)

        assert_same bundle, Bundle.registry[name]
      end

      def test_dev_bundle_render_with_raw_input
        bundle = DevModeBundle.new(FIXTURE)

        result = bundle.render('{"input":99}', raw_input: true)

        assert_equal({ 'hello' => 'world', 'input' => 99 }, result)
      end

      def test_dev_bundle_render_with_raw_output
        bundle = DevModeBundle.new(FIXTURE)

        result = bundle.render({ input: 7 }, raw_output: true)

        assert_equal '{"hello":"world","input":7}', result
      end

      def test_dev_bundle_render_chunks_with_raw_input
        chunks = []
        bundle = DevModeBundle.new(FIXTURE)

        bundle.render_chunks('{"input":3}', raw_input: true) { |chunk| chunks << chunk }

        assert_equal ['<div>hello</div>'], chunks
      end

      def test_dev_bundle_render_chunks_yields_to_block
        chunks = []
        bundle = DevModeBundle.new(FIXTURE)

        bundle.render_chunks({ input: 1 }) { |chunk| chunks << chunk }

        assert_equal ['<div>hello</div>'], chunks
      end

      def test_dev_bundle_render_chunks_returns_enumerator_without_block
        bundle = DevModeBundle.new(FIXTURE)

        enum = bundle.render_chunks({ input: 1 })

        assert_kind_of Enumerator, enum
      end

      def test_dev_bundle_resolve_alias_default_comes_from_config
        Config.dev_resolve_alias = { '@' => 'test/fixtures' }

        bundle = DevModeBundle.new(FIXTURE)

        assert_equal({ '@' => 'test/fixtures' }, bundle.instance_variable_get(:@resolve_alias))
      ensure
        Config.dev_resolve_alias = nil
      end

      def test_dev_bundle_resolve_alias_override
        custom = { 'lib' => 'src/lib' }
        bundle = DevModeBundle.new(FIXTURE, resolve_alias: custom)

        assert_equal({ 'lib' => 'src/lib' }, bundle.instance_variable_get(:@resolve_alias))
      end
    end
  end
end
