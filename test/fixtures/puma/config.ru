# frozen_string_literal: true

require 'ssr/deno'

BUNDLE_PATH = File.expand_path('../../fixtures/minimal-bundle.js', __dir__)

class PumaTestApp
  def bundle
    @bundle ||= SSR::Deno::Bundle.new(BUNDLE_PATH)
  end

  def call(_env)
    body = bundle.render({ data: { name: 'Puma' } })
    [200, { 'content-type' => 'text/html' }, [body]]
  end
end

run PumaTestApp.new
