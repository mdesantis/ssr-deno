# frozen_string_literal: true

module SSR
  module Deno
    class InstallGenerator < Rails::Generators::Base
      source_root File.expand_path('templates', __dir__)

      desc 'Creates a ssr-deno initializer and Puma config'

      def create_initializer
        template 'ssr_deno.rb', 'config/initializers/ssr_deno.rb'
      end

      def add_puma_on_worker_boot
        create_file 'config/puma.rb'

        append_to_file 'config/puma.rb' do
          "\n# ssr-deno: create bundles in each worker after fork.\n" \
            "on_worker_boot do\n  " \
            "SSR::Deno::Bundle.create_bundles!\n" \
            "end\n"
        end
      end
    end
  end
end
