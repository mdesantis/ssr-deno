# frozen_string_literal: true

# Guards integration tests that depend on a built sample bundle.
#
# Locally, a missing bundle just means `samples:build` hasn't run yet, so it
# skips with a hint. In CI — or with SSR_DENO_STRICT_SAMPLES set — a missing
# bundle fails loudly instead: `rakelib/samples.rake` only builds a sample if
# its `dist/` doesn't already exist, so a stale `dist/` from an older
# dependency set would otherwise skip silently and the run would read green
# without ever exercising the current one.
module SampleBundleGuard
  def require_sample_bundle!(path)
    return if File.exist?(path)

    message = "#{path} not found — run `bundle exec rake samples:build`"

    if ENV['CI'] || ENV['SSR_DENO_STRICT_SAMPLES']
      flunk message
    else
      skip message
    end
  end
end
