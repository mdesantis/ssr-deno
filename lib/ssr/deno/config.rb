# frozen_string_literal: true

module SSR
  module Deno
    module Config
      @_mutex = Mutex.new

      DEFAULT_DEV_RESOLVE_ALIAS = { '@' => 'app/frontend' }.freeze

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

        # rubocop:disable ThreadSafety/ClassInstanceVariable
        def dev_resolve_alias
          @dev_resolve_alias || DEFAULT_DEV_RESOLVE_ALIAS
        end
        # rubocop:enable ThreadSafety/ClassInstanceVariable

        # Setting to +nil+ restores the default alias map.
        def dev_resolve_alias=(map)
          @_mutex.synchronize do
            @dev_resolve_alias =
              map && map.transform_keys(&:to_s).transform_values(&:to_s).freeze
          end
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

          integer_value = parse_integer_env(env_var, value)

          return if integer_value.nil?

          # Separate rescue from the parse above: an out-of-range value is
          # a different failure and gets a different message.
          #
          # RangeError as well as ArgumentError: the native setters take
          # unsigned integers, so a negative value raises RangeError out of
          # magnus's conversion before any of our own validation runs. Without
          # it, SSR_DENO_ISOLATE_POOL_SIZE=-1 aborts `require "ssr/deno"`.
          begin
            send(setter, integer_value)
          rescue ArgumentError, RangeError => error
            warn "[ssr-deno] Cannot apply #{env_var}=#{value.inspect}: #{error.message}, skipping"
          end
        end

        def parse_integer_env(env_var, value)
          Integer(value)
        rescue ArgumentError => error
          warn "[ssr-deno] Cannot parse #{env_var}=#{value.inspect}: #{error.message}, skipping"
          nil
        end

        def apply_bool_env(env_var, setter)
          value = ENV.fetch(env_var, nil)

          return if value.nil? || value.empty?

          recognized = %w[true 1 yes false 0 no]

          unless recognized.include?(value.downcase)
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
