# frozen_string_literal: true

module SSR
  module Deno
    # Thin wrapper around +ActiveSupport::Notifications+ that no-ops when the
    # module is not loaded (core gem mode, no Rails).
    #
    # The +ActiveSupport::Notifications+ branch is exercised by the main test
    # suite via a mock module (see +test_instrument_with_active_support_notifications+
    # in test_deno_bundle.rb) and end-to-end by the Rails integration test
    # (+test_instrumentation_fires_bundle_miss_event+ in test_integration_deno_rails.rb).
    module Instrumenter
      class << self
        # Instrument a block with ActiveSupport::Notifications.
        # No-ops when ActiveSupport::Notifications is not loaded (core gem mode).
        def instrument(name, payload = {}, &)
          if defined?(ActiveSupport::Notifications)
            ActiveSupport::Notifications.instrument(name, payload, &)
          elsif block_given?
            yield payload
          end
        end
      end
    end
  end
end
