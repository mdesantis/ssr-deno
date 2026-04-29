# frozen_string_literal: true

module SSR
  module Deno
    class InstallGenerator < Rails::Generators::Base
      source_root File.expand_path('templates', __dir__)

      desc 'Creates a ssr-deno initializer in config/initializers/'

      def create_initializer
        template 'ssr_deno.rb', 'config/initializers/ssr_deno.rb'
      end
    end
  end
end
