# frozen_string_literal: true

require 'ssr/deno'

BUNDLE_PATH = File.expand_path('../../fixtures/minimal-bundle.js', __dir__)

# TODO: replace $bundle global with anonymous class once
# Rack::Builder do…end block nesting issue is resolved
$bundle = nil # rubocop:disable Style/GlobalVars

run lambda { |_env|
  $bundle ||= SSR::Deno::Bundle.new(BUNDLE_PATH) # rubocop:disable Style/GlobalVars
  body = $bundle.render({ data: { name: 'Puma' } }) # rubocop:disable Style/GlobalVars
  [200, { 'content-type' => 'text/html' }, [body]]
}
