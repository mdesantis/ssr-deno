# frozen_string_literal: true

require 'test_helper'
require 'tmpdir'

module SSR
  class TestDenoRender < Minitest::Test
    MINIMAL_BUNDLE = File.expand_path('../fixtures/minimal-bundle.js', __dir__)

    def setup
      @bundle = SSR::Deno::Bundle.new(MINIMAL_BUNDLE)
    end

    def test_render_returns_html
      result = @bundle.render({ data: { name: 'event-loop' } })

      assert_includes result, '<h1>event-loop</h1>'
    end

    def test_render_with_raw_output
      result = @bundle.render({ data: { name: 'raw' } }, raw_output: true)

      assert_includes result, '<h1>raw</h1>'
    end

    def test_render_with_raw_input
      result = @bundle.render({ data: { name: 'raw-input' } }.to_json, raw_input: true)

      assert_includes result, '<h1>raw-input</h1>'
    end

    def test_render_with_raw_input_and_raw_output
      data = { data: { name: 'both' } }

      result = @bundle.render(data.to_json, raw_input: true, raw_output: true)

      assert_includes result, '<h1>both</h1>'
    end

    def test_render_raises_render_error_on_promise_rejection
      bundle = with_reject_bundle
      error = assert_raises(SSR::Deno::RenderError) { bundle.render({}) }

      assert_includes error.message, 'stream-rejection'
    end

    private

    def with_reject_bundle
      dir = Dir.mktmpdir
      path = File.join(dir, 'reject-stream.js')

      File.write(path, <<~JS)
        globalThis.render = function() {
          return new Promise(function(resolve, reject) {
            setTimeout(function() { reject(new Error('stream-rejection')); }, 0);
          });
        };
      JS

      SSR::Deno::Bundle.new(path)
    end
  end
end
