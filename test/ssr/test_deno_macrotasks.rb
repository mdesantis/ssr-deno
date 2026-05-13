# frozen_string_literal: true

require 'test_helper'

module SSR
  class TestDenoMacrotasks < Minitest::Test
    def setup
      @tmp_dirs = []
    end

    def teardown
      @tmp_dirs&.each { |d| FileUtils.rm_rf(d) }
      super
    end

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

    def test_timeout_fires_during_render
      result = render_in_subprocess(ASYNC_TIMEOUT)

      assert_includes result, 'fired: true'
    end

    def test_interval_fires_during_render
      result = render_in_subprocess(ASYNC_INTERVAL)

      assert_includes result, 'fired: true'
    end

    def test_message_port_fires_during_render
      result = render_in_subprocess(ASYNC_MESSAGE_PORT)

      assert_includes result, 'received: true'
    end

    private

    def render_in_subprocess(js_code)
      with_temp_bundle(js_code).render({})
    end

    def with_temp_bundle(js_code)
      dir = Dir.mktmpdir
      @tmp_dirs << dir
      path = File.join(dir, 'bundle.js')
      File.write(path, js_code)
      SSR::Deno::Bundle.new(path)
    end
  end
end
