# frozen_string_literal: true

module SSR
  module Deno
    class Railtie < Rails::Railtie
      config.ssr_deno = ActiveSupport::OrderedOptions.new
      config.ssr_deno.bundles = {}
      config.ssr_deno.enabled = true
      config.ssr_deno.auto_reload = Rails.env.development?
      config.ssr_deno.raise_on_render_error = !Rails.env.production?
      config.ssr_deno.raise_on_bundle_error = !Rails.env.production?
      config.ssr_deno.max_heap_size_mb = nil # nil = 64 MB (default)
      config.ssr_deno.isolate_pool_size = nil # nil = 1 (default)
      config.ssr_deno.heap_stats_sample_rate = 100 # emit heap stats every N renders
      config.ssr_deno.render_timeout_ms = nil # nil = 500ms (default)
      config.ssr_deno.node_builtins_enabled = nil # nil = false (default)
      config.ssr_deno.source_maps_enabled = !Rails.env.production?

      initializer 'ssr_deno.setup' do |_app|
        ActiveSupport.on_load(:action_view) do
          include SSR::Deno::RailsHelper
        end
      end

      # Copy every non-nil +config.ssr_deno.*+ runtime option into
      # +SSR::Deno::Config+. Guarded on nil rather than truthiness: nil means
      # "leave the gem default alone", while an out-of-range value like 0 is a
      # mistake the setter should reject loudly rather than one this
      # initializer silently drops.
      RUNTIME_CONFIG_KEYS = %i[
        max_heap_size_mb isolate_pool_size render_timeout_ms
        node_builtins_enabled source_maps_enabled
      ].freeze

      def self.apply_runtime_config(ssr_deno_config)
        RUNTIME_CONFIG_KEYS.each do |key|
          value = ssr_deno_config.public_send(key)

          SSR::Deno::Config.public_send(:"#{key}=", value) unless value.nil?
        end
      end

      initializer 'ssr_deno.init_bundles', after: :load_config_initializers do |_app|
        next unless config.ssr_deno.enabled

        # Apply config before runtime initialization.
        # Must be set before any Bundle.new call (triggers pool init).
        SSR::Deno::Railtie.apply_runtime_config(config.ssr_deno)

        # Store bundle configs in +registry+. Actual +Bundle.new+ is called
        # from +on_worker_boot+ (Puma clustered) or lazily on first render
        # (single mode).
        config.ssr_deno.bundles.each do |name, path|
          unless path
            Rails.logger.error "[ssr-deno] Bundle #{name.inspect} has no path. " \
                               'Set a path in config.ssr_deno.bundles.'
            next
          end

          unless File.exist?(path)
            Rails.logger.error "[ssr-deno] Bundle #{name.inspect} not found at #{path}. Skipping."
            next
          end

          SSR::Deno::Bundle.registry[name] = { path: path, auto_reload: config.ssr_deno.auto_reload }
        end
      end

      # Subscribe a sampler that emits heap_stats.ssr_deno every +sample_rate+
      # renders. A non-positive rate disables sampling and returns +nil+
      # without subscribing — cheaper than a per-event guard on the render hot
      # path, and it makes `heap_stats_sample_rate = 0` mean "off" rather than
      # raising ZeroDivisionError out of `counter % 0` on every render.
      #
      # @return [Object, nil] the Active Support subscriber, or nil when disabled.
      def self.subscribe_heap_stats(sample_rate)
        rate = sample_rate.to_i

        return unless rate.positive?

        due = heap_stats_sampler(rate)

        ActiveSupport::Notifications.subscribe('render.ssr_deno') do |*_args|
          next unless due.call

          stats = SSR::Deno::HeapStats.fetch

          ActiveSupport::Notifications.instrument('heap_stats.ssr_deno', stats)
        rescue SSR::Deno::Error, JSON::ParserError => error
          Rails.logger.warn "[ssr-deno] Failed to collect heap stats: #{error.message}"
        end
      end

      # Thread-safe "every Nth call" predicate. The counter is shared across
      # every render thread, so the increment and the test have to happen
      # under one lock.
      def self.heap_stats_sampler(rate)
        counter = 0
        mutex = Mutex.new

        -> { mutex.synchronize { ((counter += 1) % rate).zero? } }
      end

      # Sample V8 heap stats periodically and emit heap_stats.ssr_deno events.
      initializer 'ssr_deno.heap_stats' do |_app|
        next unless config.ssr_deno.enabled

        SSR::Deno::Railtie.subscribe_heap_stats(config.ssr_deno.heap_stats_sample_rate)
      end
    end
  end
end
