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

  # Rust test sources use both the `*_test.rs` convention (alongside the
  # module) and the `<module>/tests.rs` submodule convention. All are
  # `#[cfg(test)]`-gated and never compiled into a release build, so none ship.
  # Scoped to src/ and crates/ rather than ext/ssr_deno/**: the latter walks
  # the multi-gigabyte cargo target/ tree on every gemspec evaluation, i.e.
  # every `bundle exec`.
  rust_test_files = Dir['ext/ssr_deno/{src,crates}/**/*_test.rs'] +
                    Dir['ext/ssr_deno/{src,crates}/**/tests.rs']
  spec.files = Dir['lib/**/*.rb', 'sig/**/*.rbs'] +
               (Dir['ext/ssr_deno/Cargo.*', 'ext/ssr_deno/src/**/*', 'ext/ssr_deno/crates/**/*'] -
                rust_test_files) +
               ['ssr-deno.gemspec', 'ext/ssr_deno/extconf.rb', 'README.md', 'CHANGELOG.md', 'LICENSE.txt'] +
               Dir['docs/*.md']

  # Native extension
  spec.extensions = ['ext/ssr_deno/extconf.rb']

  # Runtime dependencies
  spec.add_dependency 'rb_sys', '~> 0.9'
end
