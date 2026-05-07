# frozen_string_literal: true

module SSR
  module Deno
    class Railtie < Rails::Railtie
      config.ssr_deno = ActiveSupport::OrderedOptions.new
      config.ssr_deno.bundles = { application: nil } # name => path, nil = use default path
      config.ssr_deno.enabled = true
      config.ssr_deno.auto_reload = Rails.env.development?
      config.ssr_deno.raise_on_render_error = !Rails.env.production?
      config.ssr_deno.max_heap_size_mb = nil # nil = 64 MB (default)
      config.ssr_deno.isolate_pool_size = nil # nil = 1 (default)
      config.ssr_deno.heap_stats_sample_rate = 100 # emit heap stats every N renders
      config.ssr_deno.node_builtins_enabled = nil # nil = false (default)

      initializer 'ssr_deno.setup' do |_app|
        ActiveSupport.on_load(:action_view) do
          include SSR::Deno::Helper
        end
      end

      initializer 'ssr_deno.init_bundles', after: :load_config_initializers do |_app|
        next unless config.ssr_deno.enabled

        # Apply config before runtime initialization.
        # Must be set before any Bundle.new call (triggers pool init).
        SSR::Deno.max_heap_size_mb = config.ssr_deno.max_heap_size_mb if config.ssr_deno.max_heap_size_mb
        SSR::Deno.isolate_pool_size = config.ssr_deno.isolate_pool_size if config.ssr_deno.isolate_pool_size
        SSR::Deno.node_builtins_enabled = config.ssr_deno.node_builtins_enabled if config.ssr_deno.node_builtins_enabled

        # Store bundle configs for deferred initialization. Actual
        # Bundle.new is called from +on_worker_boot+ (Puma clustered) or
        # lazily on first render (single mode).
        config.ssr_deno.bundles.each do |name, path|
          path ||= default_bundle_path(name)

          next unless path

          unless File.exist?(path)
            Rails.logger.warn "[ssr-deno] Bundle #{name.inspect} not found at #{path}. Skipping."
            next
          end

          SSR::Deno::Bundle.deferred_bundles[name] = { path: path, auto_reload: config.ssr_deno.auto_reload }
        end
      end

      # Sample V8 heap stats periodically and emit heap_stats.ssr_deno events.
      initializer 'ssr_deno.heap_stats' do |_app|
        sample_rate = config.ssr_deno.heap_stats_sample_rate
        counter = 0
        mutex = Mutex.new

        ActiveSupport::Notifications.subscribe('render.ssr_deno') do |*_args|
          should_sample = false

          mutex.synchronize do
            counter += 1
            should_sample = (counter % sample_rate).zero?
          end

          next unless should_sample

          stats = SSR::Deno.heap_stats
          ActiveSupport::Notifications.instrument('heap_stats.ssr_deno', stats)
        rescue SSR::Deno::Error => error
          Rails.logger.warn "[ssr-deno] Failed to collect heap stats: #{error.message}"
        end
      end

      # Subscribe to ssr-deno instrumentation events for logging.
      initializer 'ssr_deno.subscribe_events' do |_app|
        ActiveSupport::Notifications.subscribe(/\.ssr_deno$/) do |name, start, finish, _id, payload|
          duration = ((finish - start) * 1000).round(2)

          if payload[:error]
            Rails.logger.warn "[ssr-deno] #{name} failed: #{payload[:error]} (#{duration}ms)"
          else
            Rails.logger.debug "[ssr-deno] #{name} completed (#{duration}ms)"
          end
        end
      end

      private

      def default_bundle_path(name)
        Rails.root.join("dist/server/#{name}/entry-server.js")
      end
    end
  end
end
