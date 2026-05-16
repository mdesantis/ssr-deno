# frozen_string_literal: true

require 'json'
require_relative 'instrumenter'

module SSR
  module Deno
    # Dev-mode bundle that loads source `.tsx` files directly into an embedded
    # Deno V8 isolate via the DevModuleLoader. No pre-build step required.
    #
    # Registers itself in {Bundle.registry} so {RailsHelper#find_bundle!}
    # resolves it transparently (same `#render` / `#render_chunks` interface
    # as {Bundle}).
    #
    # @note **Experimental — not for production.** The transpile pipeline,
    #   CJS→ESM interop shim, error formatting, and auto-reload heuristics
    #   may change without a deprecation cycle. Report issues at
    #   https://github.com/mdesantis/ssr-deno/issues. See
    #   {file:docs/dev-mode.md} for caveats and limitations.
    # @note Compiled into the gem by default (the `dev-mode` Cargo feature is
    #   on by default). Build with `--no-default-features` to strip dev-mode
    #   from a production gem; calling +DevModeBundle.new+ on such a build
    #   raises +NoMethodError+ on the missing native methods.
    # @note Source-file auto-reload is available: set +auto_reload = true+
    #   to check for changes on every render (respawning the worker on stale).
    #   Disabled by default (opt-in via +auto_reload+ accessor).
    class DevModeBundle
      # @param bundle_path [String] Path to the source entry file (.tsx/.ts).
      # @param name [Symbol, String] Registry name for +find_bundle!+ lookup.
      #   Defaults to +bundle_path+.
      # @param resolve_alias [Hash{String => String}] Path alias map, e.g.
      #   `{ '@' => 'app/frontend' }`.  Keys and values are converted to
      #   strings.  Defaults to {Config.dev_resolve_alias}.
      # @param project_root [String] Project root directory for permission
      #   boundary and `node_modules/` resolution.  Expanded to an absolute
      #   path before being handed to the native worker (relative paths fail
      #   +Url::from_file_path+ on the Rust side).  Defaults to +Dir.pwd+.
      def initialize(bundle_path, name: nil, resolve_alias: nil,
                     project_root: Dir.pwd)
        @bundle_path = bundle_path.to_s
        @resolve_alias = (resolve_alias || SSR::Deno::Config.dev_resolve_alias)
                         .transform_keys(&:to_s).transform_values(&:to_s)
        @project_root = File.expand_path(project_root.to_s)
        @name = name || @bundle_path
        @auto_reload = false
        @_bundle_mutex = Mutex.new

        create_worker
        load_entry

        Bundle.registry[@name] = self
      end

      # @return [Boolean] When enabled, checks source files for changes before
      #   each render and respawns the worker if any file was modified. Disabled
      #   by default — no change-detection overhead when unused.
      attr_reader :auto_reload, :bundle_path

      # @param value [Boolean] Enable auto-reload (mtime check before each render).
      def auto_reload=(value)
        @_bundle_mutex.synchronize { @auto_reload = value }
      end

      # Render the entry via the dev worker's full Deno event loop.
      # @param data [Hash, String] Data passed to the JS render function.
      # @param raw_input [Boolean] Skip +JSON.generate+.
      # @param raw_output [Boolean] Skip +JSON.parse+.
      def render(data = nil, raw_input: false, raw_output: false)
        reload_if_changed

        json = raw_input ? data : JSON.generate(data)

        instrument 'render.ssr_deno', bundle_name: @bundle_path, identifier: @bundle_path do |payload|
          result = SSR::Deno.native_dev_render(
            @handle, @bundle_path, json, SSR::Deno::Config.render_timeout_ms
          )

          raw_output ? result : JSON.parse(result)
        rescue StandardError => error
          payload[:error] = error.message

          raise
        end
      end

      # Chunked render. Yields HTML chunks incrementally.
      # @param data [Hash, String] Data passed to the JS render function.
      # @param raw_input [Boolean] Skip +JSON.generate+.
      # @return [Enumerator, nil] Enumerator of chunks (no block) or nil (block given).
      def render_chunks(data = nil, raw_input: false, &)
        reload_if_changed

        json = raw_input ? data : JSON.generate(data)

        instrument 'render.ssr_deno', bundle_name: @bundle_path, identifier: @bundle_path do
          SSR::Deno.native_dev_render_chunks(
            @handle, @bundle_path, json, SSR::Deno::Config.render_timeout_ms, &
          )
        end
      end

      private

      def instrument(...)
        Instrumenter.instrument(...)
      end

      # Reload guard. Triggers a fresh worker on either:
      # - any tracked source file's mtime changed (`native_dev_check_stale`), or
      # - the previous reload attempt failed (`@reload_pending`). Without this
      #   second condition a transpile error in the entry leaves the new worker
      #   with an empty mtime cache, `check_stale` returns false forever, and
      #   subsequent edits would never trigger a retry — user stuck until they
      #   restart Rails.
      def reload_if_changed
        return unless @auto_reload

        @_bundle_mutex.synchronize do
          next unless @reload_pending || SSR::Deno.native_dev_check_stale(@handle)

          create_worker
          load_entry
          @reload_pending = false
        rescue StandardError
          @reload_pending = true
          raise
        end
      end

      def create_worker
        @handle = SSR::Deno.native_dev_worker_new(
          @project_root,
          SSR::Deno::Config.max_heap_size_mb
        )
      end

      def load_entry
        SSR::Deno.native_dev_load_entry(
          @handle,
          @bundle_path,
          JSON.generate(@resolve_alias)
        )
      end
    end
  end
end
