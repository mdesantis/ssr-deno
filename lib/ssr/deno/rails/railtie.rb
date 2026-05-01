# frozen_string_literal: true

module SSR
  module Deno
    class Railtie < Rails::Railtie
      config.ssr_deno = ActiveSupport::OrderedOptions.new
      config.ssr_deno.bundles = { application: nil } # name => path, nil = use default path
      config.ssr_deno.enabled = true
      config.ssr_deno.auto_reload = Rails.env.development?
      config.ssr_deno.raise_on_render_error = !Rails.env.production?
      config.ssr_deno.max_heap_size_mb = nil   # nil = 64 MB (default)
      config.ssr_deno.isolate_pool_size = nil  # nil = auto-detect from CPU count

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

        config.ssr_deno.bundles.each do |name, path|
          path ||= default_bundle_path(name)

          next unless path

          unless File.exist?(path)
            Rails.logger.warn "[ssr-deno] Bundle #{name.inspect} not found at #{path}. Skipping."
            next
          end

          bundle = SSR::Deno::Bundle.new(path)
          bundle.auto_reload = true if config.ssr_deno.auto_reload

          SSR::Deno::Bundle.registry.register(name, bundle)
        rescue ArgumentError
          Rails.logger.warn "[ssr-deno] Bundle #{name.inspect} already registered. Skipping."
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
