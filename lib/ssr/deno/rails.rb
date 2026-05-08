# frozen_string_literal: true

# Rails integration for ssr-deno.
# Activated by: gem 'ssr_deno', require: 'ssr/deno/rails'
unless defined?(Rails::Railtie)
  raise LoadError, '[ssr-deno] Rails integration requires railties. ' \
                   'Add gem "railties" to your Gemfile.'
end

require_relative '../deno'
require_relative 'rails/railtie'
require_relative 'rails/helper'
require_relative 'rails/generators/ssr/deno/install_generator' if defined?(Rails::Generators)
