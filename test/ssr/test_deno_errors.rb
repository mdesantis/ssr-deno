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

    def test_native_load_bundle_when_bundle_not_found_raises_bundle_not_found_error
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

    # See `builder.rs` — the `create_web_worker_cb` for why this doesn't crash.
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
