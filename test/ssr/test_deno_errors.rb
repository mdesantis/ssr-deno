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

    HANG_JS = <<~JS.chomp
      globalThis.render = function() {
        let end = Date.now() + 15000;
        while (Date.now() < end) {}
        return 'timeout did not fire';
      };
    JS

    def test_render_timeout
      script = <<~RUBY
        require 'tmpdir'
        $LOAD_PATH.unshift('lib')
        require 'ssr/deno'
        Dir.mktmpdir do |dir|
          bundle_path = File.join(dir, 'hung-bundle.js')
          File.write(bundle_path, #{HANG_JS.inspect})
          bundle = SSR::Deno::Bundle.new(bundle_path)
          begin
            bundle.render({})
            exit 1
          rescue SSR::Deno::RenderError
            exit 0
          end
        end
      RUBY
      _, _, status = Open3.capture3(RbConfig.ruby, '-e', script, chdir: GEM_ROOT)

      assert_predicate status.exitstatus, :zero?, 'Expected SSR::Deno::RenderError on hung render'
    end

    def test_render_works_after_timeout
      script = <<~RUBY
        require 'tmpdir'
        $LOAD_PATH.unshift('lib')
        require 'ssr/deno'
        SSR::Deno.isolate_pool_size = 2
        Dir.mktmpdir do |dir|
          hang_path = File.join(dir, 'hang-bundle.js')
          File.write(hang_path, #{HANG_JS.inspect})
          hang_bundle = SSR::Deno::Bundle.new(hang_path)
          begin
            hang_bundle.render({})
          rescue SSR::Deno::RenderError
            # expected — hung isolate is now blocked
          end
          ok_path = File.join(dir, 'ok-bundle.js')
          File.write(ok_path, "globalThis.render = function() { return '<h1>ok</h1>'; };")
          ok_bundle = SSR::Deno::Bundle.new(ok_path)
          result = ok_bundle.render({})
          if result == '<h1>ok</h1>'
            exit 0
          else
            exit 1
          end
        end
      RUBY
      _, _, status = Open3.capture3(RbConfig.ruby, '-e', script, chdir: GEM_ROOT)

      assert_predicate status.exitstatus, :zero?, 'Expected recovery render to succeed after timeout on another isolate'
    end

    def test_render_when_worker_dies_raises_js_runtime_worker_error
      skip 'No public API to terminate the Deno worker thread from Ruby'
    end
  end
end
