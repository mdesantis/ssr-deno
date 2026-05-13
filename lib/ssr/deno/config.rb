# frozen_string_literal: true

module SSR
  module Deno
    module Config
      @_mutex = Mutex.new

      class << self
        def max_heap_size_mb=(mega_bytes)
          @_mutex.synchronize { SSR::Deno.native_set_max_heap_size_mb(mega_bytes.to_i) }
        end

        def isolate_pool_size=(size)
          @_mutex.synchronize { SSR::Deno.native_set_isolate_pool_size(size.to_i) }
        end

        def render_timeout_ms=(milliseconds)
          @_mutex.synchronize { SSR::Deno.native_set_render_timeout_ms(milliseconds.to_i) }
        end

        def node_builtins_enabled=(enabled)
          @_mutex.synchronize { SSR::Deno.native_set_node_builtins_enabled(enabled) }
        end

        def source_maps_enabled=(enabled)
          @_mutex.synchronize { SSR::Deno.native_set_source_maps_enabled(enabled) }
        end

        def max_heap_size_mb
          SSR::Deno.native_get_max_heap_size_mb
        end

        def isolate_pool_size
          SSR::Deno.native_get_isolate_pool_size
        end

        def render_timeout_ms
          SSR::Deno.native_get_render_timeout_ms
        end

        def node_builtins_enabled?
          SSR::Deno.native_get_node_builtins_enabled
        end

        def source_maps_enabled?
          SSR::Deno.native_get_source_maps_enabled
        end

        private

        def apply_env_var_defaults
          apply_integer_env('SSR_DENO_MAX_HEAP_SIZE_MB', :max_heap_size_mb=)
          apply_integer_env('SSR_DENO_ISOLATE_POOL_SIZE', :isolate_pool_size=)
          apply_integer_env('SSR_DENO_RENDER_TIMEOUT_MS', :render_timeout_ms=)
          apply_bool_env('SSR_DENO_NODE_BUILTINS_ENABLED', :node_builtins_enabled=)
          apply_bool_env('SSR_DENO_SOURCE_MAPS_ENABLED', :source_maps_enabled=)
        end

        def apply_integer_env(env_var, setter)
          value = ENV.fetch(env_var, nil)

          return if value.nil? || value.empty?

          begin
            send(setter, Integer(value))
          rescue ArgumentError => error
            warn "[ssr-deno] Cannot apply #{env_var}=#{value.inspect}: #{error.message}, skipping"
          end
        end

        def apply_bool_env(env_var, setter)
          value = ENV.fetch(env_var, nil)

          return if value.nil? || value.empty?

          recognised = %w[true 1 yes false 0 no]

          unless recognised.include?(value.downcase)
            warn "[ssr-deno] Unrecognized boolean for #{env_var}=#{value.inspect}, ignoring"
            return
          end

          bool_value = %w[true 1 yes].include?(value.downcase)

          send(setter, bool_value)
        end
      end

      apply_env_var_defaults
    end
  end
end
