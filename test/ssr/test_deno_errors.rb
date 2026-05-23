# frozen_string_literal: true

require 'test_helper'
require 'support/subprocess_helper'

module SSR
  class TestDenoErrors < Minitest::Test
    include TestFixturePaths
    include SubprocessHelper

    def setup
      @bundle = SSR::Deno::Bundle.new(MINIMAL_BUNDLE)
    end

    def test_render_when_js_throws_raises_render_error
      assert_raises(SSR::Deno::RenderError) do
        @bundle.render('invalid-json', raw_input: true)
      end
    end

    def test_bundle_initialize_when_path_not_found_raises_errno_enoent
      assert_subprocess(<<~RUBY, 'Expected Errno::ENOENT to be raised')
        begin
          SSR::Deno::Bundle.new('/nonexistent/entry-server.js')
        rescue Errno::ENOENT
          exit 0
        end
        exit 1
      RUBY
    end

    def test_native_render_when_runtime_not_initialized_raises_js_runtime_not_initialized_error
      assert_subprocess(<<~RUBY, 'Expected JsRuntimeNotInitializedError to be raised')
        begin
          SSR::Deno.native_render('some_id', '{}')
        rescue SSR::Deno::JsRuntimeNotInitializedError
          exit 0
        end
        exit 1
      RUBY
    end

    def test_source_map_resolves_error_location
      out, _, status = run_subprocess(<<~RUBY, env: { 'SSR_DENO_SOURCE_MAPS_ENABLED' => 'true' })
        require 'tmpdir'
        require 'json'

        Dir.mktmpdir do |dir|
          js_path = File.join(dir, 'bundle.js')
          File.write(js_path, <<~JS)
            globalThis.render = function() {
              throw new Error('test-error');
            };
          JS
          File.write("\#{js_path}.map", JSON.generate({
            version: 3,
            file: 'bundle.js',
            sources: ['components/thrower.tsx'],
            mappings: 'AAAA;AACA'
          }))

          bundle = SSR::Deno::Bundle.new(js_path)

          begin
            bundle.render({})
          rescue SSR::Deno::RenderError => e
            puts e.message
            exit 0
          end

          exit 1
        end
      RUBY

      assert_predicate status, :success?
      assert_includes out, 'components/thrower.tsx'
    end

    def test_source_map_disabled_preserves_raw_v8_message
      out, _, status = run_subprocess(<<~RUBY)
        require 'tmpdir'
        require 'json'

        Dir.mktmpdir do |dir|
          js_path = File.join(dir, 'bundle.js')
          File.write(js_path, <<~JS)
            globalThis.render = function() {
              throw new Error('test-error');
            };
          JS
          File.write("\#{js_path}.map", JSON.generate({
            version: 3,
            file: 'bundle.js',
            sources: ['components/thrower.tsx'],
            mappings: 'AAAA;AACA'
          }))

          bundle = SSR::Deno::Bundle.new(js_path)

          begin
            bundle.render({})
          rescue SSR::Deno::RenderError => e
            puts e.message
            exit 0
          end

          exit 1
        end
      RUBY

      assert_predicate status, :success?
      assert_includes out, 'bundle.js'
    end

    def test_source_map_missing_does_not_crash
      out, _, status = run_subprocess(<<~RUBY, env: { 'SSR_DENO_SOURCE_MAPS_ENABLED' => 'true' })
        require 'tmpdir'

        Dir.mktmpdir do |dir|
          js_path = File.join(dir, 'bundle.js')
          File.write(js_path, <<~JS)
            globalThis.render = function() {
              throw new Error('test-error');
            };
          JS

          bundle = SSR::Deno::Bundle.new(js_path)

          begin
            bundle.render({})
          rescue SSR::Deno::RenderError => e
            puts e.message
            exit 0
          end

          exit 1
        end
      RUBY

      assert_predicate status, :success?
      assert_includes out, 'bundle.js'
    end

    def test_js_error_name_returns_nil_for_plain_message
      error = SSR::Deno::RenderError.new('something went wrong')

      assert_nil error.js_error_name
    end

    def test_js_error_message_returns_raw_for_plain_message
      error = SSR::Deno::RenderError.new('plain')

      assert_equal 'plain', error.js_error_message
    end

    def test_js_error_backtrace_returns_nil_for_plain_message
      error = SSR::Deno::RenderError.new('plain')

      assert_nil error.js_error_backtrace
    end

    def test_js_error_name_on_invalid_json
      error = assert_raises(SSR::Deno::RenderError) do
        @bundle.render('invalid-json', raw_input: true)
      end

      assert_equal 'SyntaxError', error.js_error_name
    end

    def test_js_error_name_extracts_type_from_sync_throw
      out, _, status = run_subprocess(<<~RUBY)
        require 'tmpdir'

        Dir.mktmpdir do |dir|
          js_path = File.join(dir, 'bundle.js')
          File.write(js_path, <<~JS)
            globalThis.render = function() {
              throw new TypeError('expected number');
            };
          JS

          begin
            bundle = SSR::Deno::Bundle.new(js_path)
            bundle.render({})
          rescue SSR::Deno::RenderError => e
            puts e.js_error_name
            exit 0
          end

          exit 1
        end
      RUBY

      assert_predicate status, :success?
      assert_equal 'TypeError', out.strip
    end

    def test_js_error_name_extracts_type_from_async_rejection
      out, _, status = run_subprocess(<<~RUBY)
        require 'tmpdir'

        Dir.mktmpdir do |dir|
          js_path = File.join(dir, 'bundle.js')
          File.write(js_path, <<~JS)
            globalThis.render = function() {
              return Promise.reject(new RangeError('out of range'));
            };
          JS

          begin
            bundle = SSR::Deno::Bundle.new(js_path)
            bundle.render({})
          rescue SSR::Deno::RenderError => e
            puts e.js_error_name
            exit 0
          end

          exit 1
        end
      RUBY

      assert_predicate status, :success?
      assert_equal 'RangeError', out.strip
    end

    # See `builder.rs` — the `create_web_worker_cb` for why this doesn't crash.
    def test_js_error_message_extracts_from_sync_throw
      out, _, status = run_subprocess(<<~RUBY)
        require 'tmpdir'

        Dir.mktmpdir do |dir|
          js_path = File.join(dir, 'bundle.js')
          File.write(js_path, <<~JS)
            globalThis.render = function() {
              throw new TypeError('expected number');
            };
          JS

          begin
            bundle = SSR::Deno::Bundle.new(js_path)
            bundle.render({})
          rescue SSR::Deno::RenderError => e
            puts e.js_error_message
            exit 0
          end

          exit 1
        end
      RUBY

      assert_predicate status, :success?
      assert_equal 'expected number', out.strip
    end

    def test_js_error_message_extracts_from_async_rejection
      out, _, status = run_subprocess(<<~RUBY)
        require 'tmpdir'

        Dir.mktmpdir do |dir|
          js_path = File.join(dir, 'bundle.js')
          File.write(js_path, <<~JS)
            globalThis.render = function() {
              return Promise.reject(new RangeError('out of range'));
            };
          JS

          begin
            bundle = SSR::Deno::Bundle.new(js_path)
            bundle.render({})
          rescue SSR::Deno::RenderError => e
            puts e.js_error_message
            exit 0
          end

          exit 1
        end
      RUBY

      assert_predicate status, :success?
      assert_equal 'out of range', out.strip
    end

    def test_js_error_message_returns_raw_for_timeout
      out, _, status = run_subprocess(<<~RUBY)
        require 'tmpdir'

        SSR::Deno::Config.render_timeout_ms = 100
        SSR::Deno::Config.isolate_pool_size = 1

        Dir.mktmpdir do |dir|
          js_path = File.join(dir, 'hung-bundle.js')
          File.write(js_path, <<~JS)
            globalThis.render = function() {
              return new Promise(function() {});
            };
          JS

          begin
            bundle = SSR::Deno::Bundle.new(js_path)
            bundle.render({})
          rescue SSR::Deno::RenderError => e
            puts e.js_error_message
            exit 0
          end

          exit 1
        end
      RUBY

      assert_predicate status, :success?
      assert_includes out, 'timed out'
    end

    def test_js_error_message_extracts_from_non_error_throw
      out, _, status = run_subprocess(<<~RUBY)
        require 'tmpdir'

        Dir.mktmpdir do |dir|
          js_path = File.join(dir, 'bundle.js')
          File.write(js_path, <<~JS)
            globalThis.render = function() {
              throw "raw string";
            };
          JS

          begin
            bundle = SSR::Deno::Bundle.new(js_path)
            bundle.render({})
          rescue SSR::Deno::RenderError => e
            puts e.js_error_message
            exit 0
          end

          exit 1
        end
      RUBY

      assert_predicate status, :success?
      assert_equal 'raw string', out.strip
    end

    def test_js_error_backtrace_returns_frames_for_sync_throw
      out, _, status = run_subprocess(<<~RUBY)
        require 'tmpdir'

        Dir.mktmpdir do |dir|
          js_path = File.join(dir, 'bundle.js')
          File.write(js_path, <<~JS)
            globalThis.render = function() {
              throw new Error('boom');
            };
          JS

          begin
            bundle = SSR::Deno::Bundle.new(js_path)
            bundle.render({})
          rescue SSR::Deno::RenderError => e
            bt = e.js_error_backtrace
            puts bt.join("\n")
            exit 0
          end

          exit 1
        end
      RUBY

      assert_predicate status, :success?
      refute_empty out.strip
      assert_includes out, 'at '
    end

    def test_js_error_backtrace_returns_frames_for_async_rejection
      out, _, status = run_subprocess(<<~RUBY)
        require 'tmpdir'

        Dir.mktmpdir do |dir|
          js_path = File.join(dir, 'bundle.js')
          File.write(js_path, <<~JS)
            globalThis.render = function() {
              return Promise.reject(new Error('boom'));
            };
          JS

          begin
            bundle = SSR::Deno::Bundle.new(js_path)
            bundle.render({})
          rescue SSR::Deno::RenderError => e
            bt = e.js_error_backtrace
            puts bt.join("\n")
            exit 0
          end

          exit 1
        end
      RUBY

      assert_predicate status, :success?
      refute_empty out.strip
      assert_includes out, 'at '
    end

    def test_js_error_backtrace_returns_nil_for_timeout
      out, _, status = run_subprocess(<<~RUBY)
        require 'tmpdir'

        SSR::Deno::Config.render_timeout_ms = 100
        SSR::Deno::Config.isolate_pool_size = 1

        Dir.mktmpdir do |dir|
          js_path = File.join(dir, 'hung-bundle.js')
          File.write(js_path, <<~JS)
            globalThis.render = function() {
              return new Promise(function() {});
            };
          JS

          begin
            bundle = SSR::Deno::Bundle.new(js_path)
            bundle.render({})
          rescue SSR::Deno::RenderError => e
            puts e.js_error_backtrace.inspect
            exit 0
          end

          exit 1
        end
      RUBY

      assert_predicate status, :success?
      assert_equal 'nil', out.strip
    end

    def test_js_error_backtrace_returns_nil_for_non_error_throw
      out, _, status = run_subprocess(<<~RUBY)
        require 'tmpdir'

        Dir.mktmpdir do |dir|
          js_path = File.join(dir, 'bundle.js')
          File.write(js_path, <<~JS)
            globalThis.render = function() {
              throw "raw string";
            };
          JS

          begin
            bundle = SSR::Deno::Bundle.new(js_path)
            bundle.render({})
          rescue SSR::Deno::RenderError => e
            puts e.js_error_backtrace.inspect
            exit 0
          end

          exit 1
        end
      RUBY

      assert_predicate status, :success?
      assert_equal 'nil', out.strip
    end

    def test_web_worker_in_ssr_bundle_does_not_crash_process
      assert_subprocess(<<~RUBY, 'Expected JsRuntimeWorkerError from new Worker()')
        require 'tmpdir'
        Dir.mktmpdir do |dir|
          js_path = File.join(dir, 'worker-call.js')
          File.write(js_path, <<~JS)
            globalThis.render = function() {
              new Worker("data:text/javascript,", { type: "module" });
              return "<html/>";
            };
          JS

          begin
            bundle = SSR::Deno::Bundle.new(js_path)
            bundle.render({})
          rescue SSR::Deno::JsRuntimeWorkerError, SSR::Deno::RenderError
            exit 0
          end
          exit 1
        end
      RUBY
    end
  end
end
