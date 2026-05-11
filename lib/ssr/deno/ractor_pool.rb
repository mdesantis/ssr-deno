# frozen_string_literal: true

require 'json'
require_relative 'ractor_pool/worker'

module SSR
  module Deno
    # Parallel SSR via Ractors. Each Ractor runs its own GVL, so native
    # FFI calls (native_render) execute concurrently without serialization.
    # Thread-based concurrency also benefits: native_render releases the GVL
    # during its blocking channel recv (see rb_thread_call_without_gvl).
    #
    # Bypasses Bundle + ActiveSupport::Notifications (Ractor-unsafe).
    # Calls native FFI directly. native_load_bundle is idempotent
    # (overwrites __ssr_bundles[id] per isolate).
    #
    # Compatible with Ruby 3.3+ (includes both 3.x take-based Ractor API
    # and 4.0 value-based API).
    #
    # Usage:
    #   SSR::Deno::Config.isolate_pool_size = 4
    #   SSR::Deno::Config.node_builtins_enabled = true
    #   pool = SSR::Deno::RactorPool.new(bundle_path: 'dist/server/ssr.js')
    #   html = pool.render({ name: 'World' })
    #
    # Config must be set before first RactorPool.new (pool init is lazy).
    # Not compatible with SSR::Deno::Bundle — use one or the other.
    class RactorPool
      RACTOR_RESULT_METHOD = Ractor.method_defined?(:value) ? :value : :take

      def initialize(bundle_path:, size: nil, auto_reload: false)
        bundle_path = bundle_path.to_s
        @auto_reload = auto_reload
        @size = (size || 1).to_i

        @counter = -1

        init_pool(bundle_path)
        spawn_workers(bundle_path)
      end

      def size
        @workers.size
      end

      def render(data = nil, raw_input: false, raw_output: false)
        worker = next_worker
        reply = Ractor.new { Ractor.receive }

        worker.send({ type: :render, data:, raw_input:, raw_output:, reply: })
        ractor_result(reply)
      end

      def render_chunks(data = nil, raw_input: false, &block)
        worker = next_worker
        reply = Ractor.new { Ractor.receive }

        worker.send({ type: :render_chunks, data:, raw_input:, reply: })

        chunks = ractor_result(reply)

        if block
          chunks.each(&block)
          return nil
        end

        chunks
      end

      def reload
        @workers.each do |worker|
          reply = Ractor.new { Ractor.receive }

          worker.send({ type: :reload, reply: })
          ractor_result(reply)
        end
      end

      def shutdown
        @workers.each do |w|
          w.send(:shutdown)
        rescue StandardError => error
          warn "[ssr-deno] RactorPool: error during shutdown: #{error.message}"
        end
      end

      private

      def ractor_result(ractor)
        ractor.public_send(RACTOR_RESULT_METHOD)
      end

      def init_pool(bundle_path)
        SSR::Deno.native_load_bundle(bundle_path, bundle_path)
      rescue SSR::Deno::JsRuntimeInitializationError
        # Pool already initialized from prior call.
      end

      def spawn_workers(bundle_path)
        @workers = Array.new(@size) do
          Ractor.new(bundle_path, @auto_reload) do |path, auto|
            Worker.loop_body(path, auto)
          end
        end
      end

      def next_worker
        @counter = (@counter + 1) % @workers.size

        @workers[@counter]
      end
    end
  end
end
