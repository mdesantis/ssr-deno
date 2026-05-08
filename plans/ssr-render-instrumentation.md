# ssr-render-instrumentation

## Problem

`ssr_render` (the public Rails helper API) has no instrumentation event. Production monitoring relies on `render.ssr_deno` which wraps only the V8 call inside `Bundle#render`. The end-to-end timing including bundle lookup, JSON serialization/parse, and error handling is invisible.

## Changes

### 1. `lib/ssr/deno/rails/helper.rb` — update `instrument`

Mirror `Instrumenter.instrument` — pass block through so `AS::Notifications.instrument` wraps it with timing:

```ruby
def instrument(name, payload = {}, &)
  if defined?(ActiveSupport::Notifications)
    ActiveSupport::Notifications.instrument(name, payload, &)
  elsif block_given?
    yield
  end
end
```

### 2. `lib/ssr/deno/rails/helper.rb` — wrap `ssr_render` in `ssr_render.ssr_deno` event

`AS::Notifications::Instrumenter#instrument` yields the payload hash to the block, so rescues inside can set `payload[:error]`:

```ruby
def ssr_render(data = nil, **options)
  bundle_name = options.delete(:bundle) || :application

  instrument 'ssr_render.ssr_deno', bundle_name: bundle_name do |payload|
    bundle = find_bundle!(bundle_name)
    bundle.render(data, **options)
  rescue SSR::Deno::RenderError, SSR::Deno::JsRuntimeWorkerError,
         SSR::Deno::JsRuntimeOutOfMemoryError => error
    payload[:error] = error.message
    fallback_or_raise(error, bundle_name, :raise_on_render_error)
  rescue SSR::Deno::BundleNotFoundError => error
    payload[:error] = error.message
    fallback_or_raise(error, bundle_name, :raise_on_bundle_error)
  end
end
```

Behaviour per outcome:
- **Success**: event fires with timing, `payload = { bundle_name: :application }`
- **Error + raise** (dev/test): event fires with timing, `payload = { bundle_name: :application, error: "..." }`, exception re-raises
- **Error + CSR fallback** (production): event fires with timing, `payload = { bundle_name: :application, error: "..." }`, returns `''`

### 3. Existing railtie subscriber

Already subscribes to `/\.ssr_deno$/`, picks up `ssr_render.ssr_deno` for free. Logs "completed" or "failed" with timing and error message.

### 4. `test/ssr/test_integration_deno_rails.rb`

Extend `test_instrumentation_fires_bundle_miss_event` to verify `ssr_render.ssr_deno` fires with `bundle_name` and `error` in payload.

### 5. `sig/ssr/deno.rbs`

Update `instrument` signature to reflect block support.

### 6. CHANGELOG.md

Entry under Added.

## Not changing

- `render.ssr_deno` in `Bundle#render` stays (fine-grained V8 timing for deep diagnostics)
- `bundle_miss.ssr_deno` in `find_bundle!` stays (fine-grained miss tracking)
- Existing subscriber logic unchanged
