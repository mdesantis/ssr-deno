# Plan: Remove `:nocov:` from `lib/ssr/deno/bundle.rb`

## Problem

The `instrument` method in [`lib/ssr/deno/bundle.rb:69`](../lib/ssr/deno/bundle.rb:69) is wrapped in `:nocov:` because it has two code paths, and SimpleCov can only see one of them:

| Branch | Tested by | Visible to SimpleCov? |
|--------|-----------|----------------------|
| `return yield unless defined?(ActiveSupport::Notifications)` (no-op) | [`test_instrument_noop_when_active_support_notifications_not_loaded`](../test/ssr/test_deno_bundle.rb:96) | ✅ Yes |
| `ActiveSupport::Notifications.instrument(...)` (real instrumentation) | [`test_instrumentation_fires_bundle_miss_event`](../test/ssr/integration_deno_rails.rb:68) (Rails integration test) | ❌ No — excluded by `add_filter 'test/'` in [`test/test_helper.rb:8`](../test/test_helper.rb:8) |

## Solution: Extract to `SSR::Deno::Instrumenter` + test both branches in main suite

### Design

Extract the branching logic into a standalone module [`lib/ssr/deno/instrumenter.rb`](../lib/ssr/deno/instrumenter.rb). The `Bundle#instrument` method becomes a simple one-liner delegating to `Instrumenter.instrument`. The main test suite tests **both** branches of `Instrumenter` — the no-op path naturally, and the `ActiveSupport::Notifications` path by temporarily defining a mock `ActiveSupport::Notifications` module.

### Files to change

#### 1. Create `lib/ssr/deno/instrumenter.rb`

```ruby
# frozen_string_literal: true

module SSR
  module Deno
    module Instrumenter
      class << self
        # Instrument a block with ActiveSupport::Notifications.
        # No-ops when ActiveSupport::Notifications is not loaded (core gem mode).
        def instrument(name, payload = {}, &block)
          if defined?(ActiveSupport::Notifications)
            ActiveSupport::Notifications.instrument(name, payload, &block)
          else
            yield
          end
        end
      end
    end
  end
end
```

#### 2. Update `lib/ssr/deno/bundle.rb`

- Add `require_relative 'instrumenter'` at the top
- Replace the `instrument` method with a one-liner: `Instrumenter.instrument(name, payload, &)`
- Remove the `:nocov:` markers and the explanatory comment (the comment moves to `Instrumenter`)

#### 3. Update `lib/ssr/deno.rb`

- Add `require_relative 'deno/instrumenter'` (after `require_relative 'deno/bundle'`)

#### 4. Update `test/ssr/test_deno_bundle.rb`

Add a new test that exercises the `ActiveSupport::Notifications` branch:

```ruby
def test_instrument_with_active_support_notifications
  events = []
  mock_notifications = Module.new do
    define_singleton_method(:instrument) do |name, payload = {}, &block|
      events << name
      block.call
    end
  end

  Object.const_set(:ActiveSupport, Module.new) unless defined?(ActiveSupport)
  ActiveSupport.const_set(:Notifications, mock_notifications) unless ActiveSupport.const_defined?(:Notifications, false)

  @bundle.send(:instrument, 'test.ssr_deno', {}) { 'result' }

  assert_includes events, 'test.ssr_deno'
ensure
  if defined?(ActiveSupport) && ActiveSupport.const_defined?(:Notifications, false)
    ActiveSupport.send(:remove_const, :Notifications)
  end
end
```

### What stays the same

- The Rails integration test (`test/ssr/integration_deno_rails.rb`) continues to work unchanged — it exercises the real `ActiveSupport::Notifications` end-to-end.
- The existing `test_instrument_noop_when_active_support_notifications_not_loaded` test continues to cover the no-op path.
- The `:nocov:` on the Rails-specific files (`lib/ssr/deno/rails.rb`, `lib/ssr/deno/rails/`) remains untouched — those are filtered by SimpleCov anyway.

### Verification

After implementation, run:
```
bundle exec rake test
```

And verify that SimpleCov reports 100% line and branch coverage without any `:nocov:` markers in `lib/ssr/deno/bundle.rb`.
