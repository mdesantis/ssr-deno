# frozen_string_literal: true

require 'test_helper'
require 'tmpdir'

module SSR
  class TestDenoRenderChunks < Minitest::Test
    CHUNKED_BUNDLE = File.expand_path('../fixtures/chunked-bundle.js', __dir__)

    def setup
      @bundle = SSR::Deno::Bundle.new(CHUNKED_BUNDLE)
    end

    def test_render_chunks_with_block_yields_chunks
      chunks = []

      @bundle.render_chunks({ data: { name: 'chunked' } }) { |chunk| chunks << chunk }

      assert_equal '<html><body>', chunks[0]
      assert_equal '<h1>chunked</h1>', chunks[1]
      assert_equal '</body></html>', chunks[2]
    end

    def test_render_chunks_without_block_returns_enumerator
      enum = @bundle.render_chunks({ data: { name: 'enum' } })

      assert_kind_of Enumerator, enum
      chunks = enum.to_a

      assert_equal '<html><body>', chunks[0]
      assert_equal '<h1>enum</h1>', chunks[1]
      assert_equal '</body></html>', chunks[2]
    end

    def test_render_chunks_with_raw_input
      json = { data: { name: 'raw' } }.to_json
      chunks = []

      @bundle.render_chunks(json, raw_input: true) { |chunk| chunks << chunk }

      assert_equal '<h1>raw</h1>', chunks[1]
    end

    def test_render_chunks_raises_on_promise_rejection
      bundle = with_reject_bundle
      error = assert_raises(SSR::Deno::RenderError) do
        bundle.render_chunks({}) { |_chunk| nil }
      end

      assert_includes error.message, 'chunked-rejection'
    end

    def test_render_chunks_enumerator_raises_on_promise_rejection
      bundle = with_reject_bundle
      error = assert_raises(SSR::Deno::RenderError) do
        bundle.render_chunks({}).to_a
      end

      assert_includes error.message, 'chunked-rejection'
    end

    def test_render_chunks_raises_on_timeout
      bundle = with_hang_bundle

      assert_raises(SSR::Deno::RenderError) do
        bundle.render_chunks({}) { |_chunk| nil }
      end
    end

    def test_render_chunks_auto_reload
      @bundle.auto_reload = true
      chunks = []

      @bundle.render_chunks({ data: { name: 'reload' } }) { |chunk| chunks << chunk }

      assert_includes chunks[1], 'reload'
    end

    private

    def with_reject_bundle
      dir = Dir.mktmpdir
      path = File.join(dir, 'reject-chunked.js')

      File.write(path, <<~JS)
        globalThis.render = function() {
          return new Promise(function(resolve, reject) {
            setTimeout(function() { reject(new Error('chunked-rejection')); }, 0);
          });
        };
      JS

      SSR::Deno::Bundle.new(path)
    end

    def with_hang_bundle
      dir = Dir.mktmpdir
      path = File.join(dir, 'hang-chunked.js')

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
