# frozen_string_literal: true

require_relative '../test_helper'
require_relative '../support/temp_bundle_helper'

module SSR
  module Deno
    class TestDevModeBundle < Minitest::Test
      prepend TempBundleHelper

      ROOT = File.expand_path('../..', __dir__)
      FIXTURE = File.join(ROOT, 'test', 'fixtures', 'dev-entry.tsx')
      # Counter lives in module-local closure: persists across renders within
      # the same worker, resets to 0 when the module is re-evaluated by a
      # fresh V8 isolate (i.e. on auto-reload). Without the per-render
      # increment we couldn't distinguish reload from same-worker behavior.
      COUNTER_TSX = <<~JS
        let count = 0;
        globalThis.render = function () {
          count++;
          return { count: count };
        };
      JS

      def setup
        # Each test creates its own DevModeBundle and registers it. Reset the
        # shared registry so stale entries from prior tests don't keep their
        # worker threads alive past the test that owns them.
        Bundle.registry.clear
      end

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

      def test_dev_bundle_create_bundles_skips_dev_mode_entries
        bundle = DevModeBundle.new(FIXTURE)
        Bundle.registry[:app] = bundle
        # Use a .js fixture for the production Bundle path (no transpiler).
        js_fixture = File.join(ROOT, 'test', 'fixtures', 'dev-entry.js')
        Bundle.registry[:config_based] = { path: js_fixture }
        File.write(js_fixture, "globalThis.render = function() { return 'ok'; };")

        Bundle.create_bundles!

        assert_same bundle, Bundle.registry[:app]
        assert_kind_of Bundle, Bundle.registry[:config_based]
      ensure
        FileUtils.rm_f(js_fixture)
      end

      def test_dev_bundle_resolve_alias_override
        custom = { 'lib' => 'src/lib' }
        bundle = DevModeBundle.new(FIXTURE, resolve_alias: custom)

        assert_equal({ 'lib' => 'src/lib' }, bundle.instance_variable_get(:@resolve_alias))
      end

      def test_dev_bundle_bundle_path_reader
        bundle = DevModeBundle.new(FIXTURE)

        assert_equal FIXTURE, bundle.bundle_path
      end

      def test_dev_bundle_auto_reload_defaults_to_false
        bundle = DevModeBundle.new(FIXTURE)

        refute bundle.auto_reload
      end

      def test_dev_bundle_auto_reload_setter
        bundle = DevModeBundle.new(FIXTURE)

        bundle.auto_reload = true

        assert bundle.auto_reload
      end

      def test_dev_bundle_render_error_instrumentation
        bundle = DevModeBundle.new(FIXTURE)
        # Cause a JS SyntaxError by passing invalid JSON as raw input.
        # The render function calls JSON.parse(data) which throws, and the
        # error propagates through native_dev_render as RenderError.
        assert_raises SSR::Deno::RenderError do
          bundle.render('not-json{', raw_input: true)
        end
      end

      def test_dev_bundle_auto_reload_disabled_no_check
        temp_entry = File.join(temp_dir, 'entry.tsx')
        File.write(temp_entry, <<~JS)
          globalThis.render = function() {
            return { value: 1 };
          };
        JS

        bundle = DevModeBundle.new(temp_entry)

        assert_equal({ 'value' => 1 }, bundle.render)

        File.write(temp_entry, <<~JS)
          globalThis.render = function() {
            return { value: 2 };
          };
        JS

        # auto_reload defaults to false — render must still return 1
        assert_equal({ 'value' => 1 }, bundle.render)
      end

      def test_dev_bundle_auto_reload_detects_file_change
        temp_entry = File.join(temp_dir, 'entry.tsx')
        File.write(temp_entry, <<~JS)
          globalThis.render = function() {
            return { version: 1 };
          };
        JS

        bundle = DevModeBundle.new(temp_entry)
        bundle.auto_reload = true

        assert_equal({ 'version' => 1 }, bundle.render)

        # Modify the file with a future mtime to avoid filesystem resolution
        # issues on coarse-grained timers (ext3, FAT, Docker overlay).
        File.write(temp_entry, <<~JS)
          globalThis.render = function() {
            return { version: 2 };
          };
        JS
        new_time = Time.now + 1
        File.utime(new_time, new_time, temp_entry)

        assert_equal({ 'version' => 2 }, bundle.render)
      end

      def test_dev_bundle_auto_reload_uses_fresh_v8_context
        temp_entry = File.join(temp_dir, 'counter.tsx')
        File.write(temp_entry, COUNTER_TSX)

        bundle = DevModeBundle.new(temp_entry)
        bundle.auto_reload = true

        # Same worker: closure-local counter persists between renders.
        assert_equal({ 'count' => 1 }, bundle.render)
        assert_equal({ 'count' => 2 }, bundle.render)

        # Touch the file with a future mtime — auto-reload fires, fresh V8
        # isolate re-evaluates the module, counter resets to 0 then ++ to 1.
        File.write(temp_entry, COUNTER_TSX)
        new_time = Time.now + 1
        File.utime(new_time, new_time, temp_entry)

        assert_equal({ 'count' => 1 }, bundle.render)
      end

      def test_dev_bundle_auto_reload_retries_after_failed_reload
        temp_entry = File.join(temp_dir, 'entry.tsx')
        File.write(temp_entry, <<~JS)
          globalThis.render = function () { return { status: 'ok' }; };
        JS

        bundle = DevModeBundle.new(temp_entry)
        bundle.auto_reload = true

        assert_equal({ 'status' => 'ok' }, bundle.render)

        # Introduce a parse error and bump mtime — reload triggers, transpile
        # fails, the new worker's mtime cache stays empty.
        File.write(temp_entry, 'this is not valid typescript {{{')
        broken_time = Time.now + 1
        File.utime(broken_time, broken_time, temp_entry)
        assert_raises SSR::Deno::JsRuntimeInitializationError do
          bundle.render
        end

        # Fix the file. Without the retry flag, an empty mtime cache would
        # make `check_stale` return false forever — user would be stuck.
        File.write(temp_entry, <<~JS)
          globalThis.render = function () { return { status: 'recovered' }; };
        JS
        fixed_time = Time.now + 2
        File.utime(fixed_time, fixed_time, temp_entry)

        assert_equal({ 'status' => 'recovered' }, bundle.render)
      end
    end
  end
end
