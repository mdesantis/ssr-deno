# frozen_string_literal: true

require_relative '../test_helper'

module SSR
  module Deno
    class TestRactorPool < Minitest::Test
      RACTOR_RESULT_METHOD = Ractor.method_defined?(:value) ? :value : :take

      BUNDLE_PATH = File.expand_path('../../samples/vite-ssr-app/dist/server/entry-server.js', __dir__)

      def setup
        skip 'Ractor not available' unless defined?(Ractor)
        skip "#{BUNDLE_PATH} not found — run `bundle exec rake samples:build` first" unless File.exist?(BUNDLE_PATH)
      end

      def test_render_basic
        pool = RactorPool.new(bundle_path: BUNDLE_PATH, size: 1)
        result = pool.render({ name: 'RactorPool' })

        assert_includes result, 'RactorPool'
      ensure
        pool&.shutdown
      end

      def test_render_returns_parsed_json
        pool = RactorPool.new(bundle_path: BUNDLE_PATH, size: 1)
        result = pool.render({ name: 'test' })

        assert_kind_of String, result
      ensure
        pool&.shutdown
      end

      def test_round_robin_distribution
        pool = RactorPool.new(bundle_path: BUNDLE_PATH, size: 2)
        results = Array.new(4) { pool.render({ name: 'rr' }) }

        assert_equal 4, results.size
        results.each { |r| assert_includes r, 'rr' }
      ensure
        pool&.shutdown
      end

      def test_render_chunks_returns_array
        pool = RactorPool.new(bundle_path: BUNDLE_PATH, size: 1)
        chunks = pool.render_chunks({ name: 'chunks' })

        assert_kind_of Array, chunks
      ensure
        pool&.shutdown
      end

      def test_render_chunks_with_block
        pool = RactorPool.new(bundle_path: BUNDLE_PATH, size: 1)
        collected = []
        pool.render_chunks({ name: 'block' }) { |c| collected << c }

        assert_kind_of Array, collected
      ensure
        pool&.shutdown
      end

      def test_reload
        pool = RactorPool.new(bundle_path: BUNDLE_PATH, size: 2)
        result = pool.render({ name: 'before' })

        assert_includes result, 'before'
        pool.reload
        result = pool.render({ name: 'after' })

        assert_includes result, 'after'
      ensure
        pool&.shutdown
      end

      def test_raw_input_output
        pool = RactorPool.new(bundle_path: BUNDLE_PATH, size: 1)
        json = '{"name":"raw"}'
        result = pool.render(json, raw_input: true, raw_output: true)

        assert_kind_of String, result
      ensure
        pool&.shutdown
      end

      def test_pool_size
        pool = RactorPool.new(bundle_path: BUNDLE_PATH, size: 3)

        assert_equal 3, pool.size
      ensure
        pool&.shutdown
      end

      def test_concurrent_renders
        pool = RactorPool.new(bundle_path: BUNDLE_PATH, size: 4)
        ractors = Array.new(8) do
          Ractor.new(pool) do |p|
            p.render({ name: "conc#{Ractor.current.object_id}" })
          end
        end
        results = ractors.map { |r| r.public_send(RACTOR_RESULT_METHOD) }

        assert_equal 8, results.size
        results.each { |r| assert_kind_of String, r }
      ensure
        pool&.shutdown
      end
    end
  end
end
