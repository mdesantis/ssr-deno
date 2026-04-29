# frozen_string_literal: true

module SSR
  module Deno
    class Bundle
      class Registry
        include Enumerable

        def initialize
          @bundles = {}
          @mutex = Mutex.new
        end

        # Lookup a registered bundle by name.
        # @param name [Symbol] bundle name (:application by default)
        # @return [SSR::Deno::Bundle, nil]
        def [](name = :application)
          @mutex.synchronize { @bundles[name] }
        end
        alias bundle []

        # Register a named bundle.
        # @param name [Symbol]
        # @param bundle [SSR::Deno::Bundle]
        # @raise [ArgumentError] if name already registered
        def register(name, bundle)
          @mutex.synchronize do
            raise ArgumentError, "Bundle #{name.inspect} already registered" if @bundles.key?(name)

            @bundles[name] = bundle
          end
        end

        # Replace a registered bundle (for dev reload).
        # @param name [Symbol]
        # @param bundle [SSR::Deno::Bundle]
        def replace(name, bundle)
          @mutex.synchronize { @bundles[name] = bundle }
        end

        # Remove a registered bundle.
        # @param name [Symbol]
        def remove(name)
          @mutex.synchronize { @bundles.delete(name) }
        end

        # Iterate over registered bundles.
        def each(&block)
          @mutex.synchronize { @bundles.each(&block) }
        end

        # Number of registered bundles.
        def size
          @mutex.synchronize { @bundles.size }
        end
      end
    end
  end
end
