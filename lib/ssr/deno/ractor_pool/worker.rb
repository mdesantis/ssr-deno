# frozen_string_literal: true

module SSR
  module Deno
    class RactorPool
      module Worker
        module_function

        def loop_body(path, auto, esm: false)
          bundle_id = path
          mtime = auto ? File.mtime(path) : nil

          loop do
            mtime = maybe_reload(path, auto, mtime, bundle_id, esm)
            msg = Ractor.receive
            sig = dispatch(msg, bundle_id, path, esm)

            break if sig == :shutdown

            mtime = File.mtime(path) if sig == :reload
          end
        end

        def maybe_reload(path, auto, mtime, bundle_id, esm)
          return mtime unless auto

          cur = File.mtime(path)

          return mtime unless cur > mtime

          SSR::Deno.native_load_bundle(bundle_id, path, esm)
          cur
        end

        def dispatch(msg, bundle_id, path, esm)
          case msg
          in { type: :render, raw_input:, data:, raw_output:, reply: }
            handle_render(raw_input, data, raw_output, reply, bundle_id)
          in { type: :render_chunks, raw_input:, data:, reply: }
            handle_render_chunks(raw_input, data, reply, bundle_id)
          in { type: :reload, reply: }
            handle_reload(reply, bundle_id, path, esm)
          in :shutdown
            :shutdown
          end
        end

        def handle_render(raw_input, data, raw_output, reply, bundle_id)
          json_input = raw_input ? data : JSON.generate(data)
          result = SSR::Deno.native_render(bundle_id, json_input)

          reply.send(raw_output ? result : JSON.parse(result))
        end

        def handle_render_chunks(raw_input, data, reply, bundle_id)
          json_input = raw_input ? data : JSON.generate(data)
          chunks = []

          SSR::Deno.native_render_chunks(bundle_id, json_input) { |c| chunks << c }
          reply.send(chunks)
        end

        def handle_reload(reply, bundle_id, path, esm)
          SSR::Deno.native_load_bundle(bundle_id, path, esm)
          reply.send(:ok)
          :reload
        end
      end
      private_constant :Worker
    end
  end
end
