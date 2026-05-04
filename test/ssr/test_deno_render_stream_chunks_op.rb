# frozen_string_literal: true

require 'test_helper'
require 'tmpdir'

module SSR
  class TestDenoRenderStreamChunksOp < Minitest::Test
    CHUNKED_BUNDLE_OP = File.expand_path('../fixtures/chunked-stream-bundle-op.js', __dir__)

    def setup
      @bundle = SSR::Deno::Bundle.new(CHUNKED_BUNDLE_OP)
    end

    def test_render_stream_chunks_op_with_block_yields_chunks
      chunks = []

      @bundle.render_stream_chunks_op({ data: { name: 'op-chunked' } }) { |chunk| chunks << chunk }

      assert_equal '<html><body>', chunks[0]
      assert_equal '<h1>op-chunked</h1>', chunks[1]
      assert_equal '</body></html>', chunks[2]
    end

    def test_render_stream_chunks_op_without_block_returns_enumerator
      enum = @bundle.render_stream_chunks_op({ data: { name: 'op-enum' } })

      assert_kind_of Enumerator, enum
      chunks = enum.to_a

      assert_equal '<html><body>', chunks[0]
      assert_equal '<h1>op-enum</h1>', chunks[1]
      assert_equal '</body></html>', chunks[2]
    end

    def test_render_stream_chunks_op_with_raw_input
      json = { data: { name: 'op-raw' } }.to_json
      chunks = []

      @bundle.render_stream_chunks_op(json, raw_input: true) { |chunk| chunks << chunk }

      assert_equal '<h1>op-raw</h1>', chunks[1]
    end

    def test_render_stream_chunks_op_raises_on_promise_rejection
      bundle = with_reject_bundle

      error = assert_raises(SSR::Deno::RenderError) do
        bundle.render_stream_chunks_op({}) { |_chunk| nil }
      end

      assert_includes error.message, 'op-rejection'
    end

    def test_render_stream_chunks_op_raises_on_timeout
      bundle = with_hang_bundle

      assert_raises(SSR::Deno::RenderError) do
        bundle.render_stream_chunks_op({}) { |_chunk| nil }
      end
    end

    private

    def with_reject_bundle
      dir = Dir.mktmpdir
      path = File.join(dir, 'reject-op-chunked.js')

      File.write(path, <<~JS)
        globalThis.render = function() {
          return new Promise(function(resolve, reject) {
            setTimeout(function() { reject(new Error('op-rejection')); }, 0);
          });
        };
      JS

      SSR::Deno::Bundle.new(path)
    end

    def with_hang_bundle
      dir = Dir.mktmpdir
      path = File.join(dir, 'hang-op-chunked.js')

      File.write(path, <<~JS)
        globalThis.render = function() {
          return new Promise(function() {
            // Never resolves — should trigger timeout
          });
        };
      JS

      SSR::Deno::Bundle.new(path)
    end
  end
end
