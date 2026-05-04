# frozen_string_literal: true

require 'test_helper'
require 'open3'
require 'rbconfig'

module SSR
  class TestDenoMacrotasks < Minitest::Test
    GEM_ROOT = File.expand_path('../..', __dir__)

    BOOTSTRAP = <<~RUBY.freeze
      require 'tmpdir'
      $LOAD_PATH.unshift('#{File.join(GEM_ROOT, 'lib')}')
      require 'ssr/deno'
      SSR::Deno.isolate_pool_size = 1
      SSR::Deno.render_timeout_ms = 2000
    RUBY

    SYNC_TIMEOUT = <<~JS
      var fired = false;
      setTimeout(function() { fired = true; }, 0);
      globalThis.render = function() { return 'fired: ' + fired; };
    JS

    SYNC_INTERVAL = <<~JS
      var fired = false;
      var id = setInterval(function() { fired = true; clearInterval(id); }, 0);
      globalThis.render = function() { return 'fired: ' + fired; };
    JS

    SYNC_MESSAGE_PORT = <<~JS
      var received = false;
      var channel = new MessageChannel();
      channel.port1.onmessage = function() { received = true; };
      channel.port2.postMessage('hello');
      globalThis.render = function() { return 'received: ' + received; };
    JS

    # Async bundles: return a Promise that resolves after the event loop fires.
    # Used to test that event_loop: true dispatches the macrotask before the result.

    ASYNC_TIMEOUT = <<~JS
      var fired = false;
      setTimeout(function() { fired = true; }, 0);
      globalThis.render = function() {
        return new Promise(function(resolve) {
          setTimeout(function() { resolve('fired: ' + fired); }, 0);
        });
      };
    JS

    ASYNC_INTERVAL = <<~JS
      var fired = false;
      var id = setInterval(function() { fired = true; clearInterval(id); }, 0);
      globalThis.render = function() {
        return new Promise(function(resolve) {
          setTimeout(function() { resolve('fired: ' + fired); }, 0);
        });
      };
    JS

    ASYNC_MESSAGE_PORT = <<~JS
      var received = false;
      var channel = new MessageChannel();
      channel.port1.onmessage = function() { received = true; };
      channel.port2.postMessage('hello');
      globalThis.render = function() {
        return new Promise(function(resolve) {
          setTimeout(function() { resolve('received: ' + received); }, 0);
        });
      };
    JS

    def test_timeout_does_not_fire_with_default_render
      result = render_sync(SYNC_TIMEOUT)

      assert_includes result, 'fired: false'
    end

    def test_timeout_fires_with_event_loop
      result = render_async(ASYNC_TIMEOUT, event_loop: true)

      assert_includes result, 'fired: true'
    end

    def test_timeout_fires_with_render_stream
      result = render_async(ASYNC_TIMEOUT, stream: true)

      assert_includes result, 'fired: true'
    end

    def test_interval_does_not_fire_with_default_render
      result = render_sync(SYNC_INTERVAL)

      assert_includes result, 'fired: false'
    end

    def test_interval_fires_with_event_loop
      result = render_async(ASYNC_INTERVAL, event_loop: true)

      assert_includes result, 'fired: true'
    end

    def test_message_port_does_not_fire_with_default_render
      result = render_sync(SYNC_MESSAGE_PORT)

      assert_includes result, 'received: false'
    end

    def test_message_port_fires_with_event_loop
      result = render_async(ASYNC_MESSAGE_PORT, event_loop: true)

      assert_includes result, 'received: true'
    end

    private

    def render_sync(js_code)
      with_temp_bundle(js_code) { |b| b.render({}) }
    end

    def render_async(js_code, event_loop: false, stream: false)
      with_temp_bundle(js_code) do |b|
        if stream
          b.render_stream({})
        else
          b.render({}, event_loop: event_loop)
        end
      end
    end

    def with_temp_bundle(js_code)
      Dir.mktmpdir do |dir|
        path = File.join(dir, 'bundle.js')
        File.write(path, js_code)
        yield SSR::Deno::Bundle.new(path)
      end
    end
  end
end
