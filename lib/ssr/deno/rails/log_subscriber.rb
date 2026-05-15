# frozen_string_literal: true

require 'active_support/log_subscriber'

module SSR
  module Deno
    class LogSubscriber < ActiveSupport::LogSubscriber
      def render(event)
        debug { "[ssr-deno] #{identifier(event)} render completed (#{event.duration.round(2)}ms)" }
      end
      subscribe_log_level :render, :debug

      def bundle_load(event)
        debug { "[ssr-deno] #{identifier(event)} loaded (#{event.duration.round(2)}ms)" }
      end
      subscribe_log_level :bundle_load, :debug

      def ssr_render(event)
        if event.payload[:error]
          error "[ssr-deno] #{identifier(event)} failed: #{event.payload[:error]} (#{event.duration.round(2)}ms)"
        else
          debug { "[ssr-deno] #{identifier(event)} completed (#{event.duration.round(2)}ms)" }
        end
      end

      def bundle_miss(event)
        debug { "[ssr-deno] #{identifier(event)} not found" }
      end
      subscribe_log_level :bundle_miss, :debug

      def heap_stats(event)
        debug { "[ssr-deno] Heap stats: #{event.payload.inspect}" }
      end
      subscribe_log_level :heap_stats, :debug

      private

      def identifier(event)
        event.payload[:identifier] || event.payload[:bundle_name]
      end
    end
  end
end

SSR::Deno::LogSubscriber.attach_to :ssr_deno
