# frozen_string_literal: true

require_relative 'lib/ssr/deno/version'

Gem::Specification.new do |spec|
  spec.name = 'ssr-deno'
  spec.version = SSR::Deno::VERSION
  spec.authors = ['Maurizio De Santis']
  spec.email = ['desantis.maurizio@gmail.com']
  spec.summary = 'Server-side rendering for Ruby using Deno'
  spec.description = <<~TXT
    ssr-deno embeds a Deno V8 runtime in Ruby via a Rust native extension,
    enabling server-side rendering of JavaScript/TypeScript frameworks
    (React, Vue, etc.) directly from Ruby.
  TXT
  spec.homepage = 'https://github.com/mdesantis/ssr-deno'
  spec.license = 'MIT'
  spec.required_ruby_version = '>= 3.3.0'
  spec.metadata['allowed_push_host'] = 'https://rubygems.org'
  spec.metadata['homepage_uri'] = spec.homepage
  spec.metadata['source_code_uri'] = 'https://github.com/mdesantis/ssr-deno'
  spec.metadata['changelog_uri'] = 'https://github.com/mdesantis/ssr-deno/blob/main/CHANGELOG.md'
  spec.metadata['rubygems_mfa_required'] = 'true'

  spec.require_paths = ['lib']

  spec.files = `git ls-files -z`.split("\x0").reject do |f|
    f.match(%r{^(test|tmp|plans|coverage|docs|samples)/}) ||
      f.match(/^\./) ||
      f.start_with?('vendor/rusty_v8') ||
      f.match(%r{^(AGENTS|Gemfile|Rakefile|opencode|package|bin/|rakelib/)}) ||
      f.end_with?('.json')
  end

  # Native extension
  spec.extensions = ['ext/ssr_deno/extconf.rb']

  # Runtime dependencies
  spec.add_dependency 'railties' # optional — only loaded when require: 'ssr/deno/rails'
  spec.add_dependency 'rb_sys', '~> 0.9'
end
