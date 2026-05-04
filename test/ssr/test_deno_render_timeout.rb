# frozen_string_literal: true

require 'test_helper'
require 'support/subprocess_helper'

module SSR
  class TestDenoRenderTimeout < Minitest::Test
    include SubprocessHelper

    HANG_JS = <<~JS.chomp
      globalThis.render = function() {
        let end = Date.now() + 500;
        while (Date.now() < end) {}
        return 'timeout did not fire';
      };
    JS

    def test_render_timeout_raises_render_error
      assert_subprocess(<<~RUBY, 'Expected SSR::Deno::RenderError on hung render')
        require 'tmpdir'
        SSR::Deno.render_timeout_ms = 200
        SSR::Deno.isolate_pool_size = 1
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
    end

    def test_render_timeout_respects_configured_value
      slow_js = <<~JS.chomp
        globalThis.render = function() {
          let end = Date.now() + 400;
          while (Date.now() < end) {}
          return 'timeout did not fire';
        };
      JS
      assert_subprocess(<<~RUBY, 'Expected timeout at ~100ms')
        require 'tmpdir'
        SSR::Deno.render_timeout_ms = 100
        SSR::Deno.isolate_pool_size = 1
        Dir.mktmpdir do |dir|
          bundle_path = File.join(dir, 'hung-bundle.js')
          File.write(bundle_path, #{slow_js.inspect})
          bundle = SSR::Deno::Bundle.new(bundle_path)
          start = Time.now
          begin
            bundle.render({})
            exit 1
          rescue SSR::Deno::RenderError
            elapsed_ms = ((Time.now - start) * 1000).to_i
            if elapsed_ms >= 80 && elapsed_ms <= 500
              exit 0
            else
              exit 2
            end
          end
        end
      RUBY
    end

    def test_render_works_after_timeout
      assert_subprocess(<<~RUBY, 'Expected recovery render to succeed after timeout on another isolate')
        require 'tmpdir'
        SSR::Deno.render_timeout_ms = 200
        SSR::Deno.isolate_pool_size = 2
        Dir.mktmpdir do |dir|
          hang_path = File.join(dir, 'hang-bundle.js')
          File.write(hang_path, #{HANG_JS.inspect})
          hang_bundle = SSR::Deno::Bundle.new(hang_path)
          begin
            hang_bundle.render({})
          rescue SSR::Deno::RenderError
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
    end
  end
end
