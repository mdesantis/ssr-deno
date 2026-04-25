# frozen_string_literal: true

require_relative 'lib/ssr/deno/version'

Gem::Specification.new do |spec|
  spec.name = 'ssr-deno'
  spec.version = SSR::Deno::VERSION
  spec.authors = ['Maurizio De Santis']
  spec.email = ['desantis.maurizio@gmail.com']

  spec.summary = 'TODO: Write a short summary, because RubyGems requires one.'
  spec.description = 'TODO: Write a longer description or delete this line.'
  spec.homepage = 'https://github.com/mdesantis/ssr-deno'
  spec.license = 'MIT'
  spec.required_ruby_version = '>= 3.3.0'
  spec.metadata['allowed_push_host'] = "TODO: Set to your gem server 'https://example.com'"
  spec.metadata['homepage_uri'] = spec.homepage
  spec.metadata['source_code_uri'] = 'https://github.com/mdesantis/ssr-deno'
  spec.metadata['changelog_uri'] = 'https://github.com/mdesantis/ssr-deno/blob/main/CHANGELOG.md'
  spec.metadata['rubygems_mfa_required'] = 'true'

  spec.require_paths = ['lib']

  # Native extension
  spec.extensions = ['ext/ssr_deno/extconf.rb']

  # Runtime dependencies
  spec.add_dependency 'rb_sys', '~> 0.9'
end
