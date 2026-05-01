# frozen_string_literal: true

require 'test_helper'
require 'open3'
require 'rbconfig'

module SSR
  class TestDenoErrors < Minitest::Test
    BUNDLE_PATH = File.expand_path('../../samples/vite-ssr-app/dist/server/entry-server.js', __dir__)
    GEM_ROOT = File.expand_path('../..', __dir__)

    def setup
      @bundle = SSR::Deno::Bundle.new(BUNDLE_PATH)
    end

    def test_render_when_js_throws_raises_render_error
      assert_raises(SSR::Deno::RenderError) do
        @bundle.render('invalid-json', raw_input: true)
      end
    end

    def test_native_load_bundle_when_bundle_not_found_raises_bundle_not_found_error
      script = <<~RUBY
        $LOAD_PATH.unshift('lib')
        require 'ssr/deno'
        begin
          SSR::Deno::Bundle.new('/nonexistent/entry-server.js')
        rescue Errno::ENOENT
          exit 0
        end
        exit 1
      RUBY
      _, _, status = Open3.capture3(RbConfig.ruby, '-e', script, chdir: GEM_ROOT)

      assert_predicate status.exitstatus, :zero?, 'Expected Errno::ENOENT to be raised'
    end

    def test_native_render_when_runtime_not_initialized_raises_js_runtime_not_initialized_error
      script = <<~RUBY
        $LOAD_PATH.unshift('lib')
        require 'ssr/deno'
        begin
          SSR::Deno.native_render('some_id', '{}')
        rescue SSR::Deno::JsRuntimeNotInitializedError
          exit 0
        end
        exit 1
      RUBY
      _, _, status = Open3.capture3(RbConfig.ruby, '-e', script, chdir: GEM_ROOT)

      assert_predicate status.exitstatus, :zero?, 'Expected JsRuntimeNotInitializedError to be raised'
    end

    def test_render_when_worker_dies_raises_js_runtime_worker_error
      skip 'No public API to terminate the Deno worker thread from Ruby'
    end
  end
end
