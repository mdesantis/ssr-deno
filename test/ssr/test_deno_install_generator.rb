# frozen_string_literal: true

require 'test_helper'
require 'rails/generators/test_case'
require 'ssr/deno/rails/generators/ssr/deno/install_generator'

module Ssr
  module Deno
    class TestInstallGenerator < Rails::Generators::TestCase
      tests InstallGenerator
      destination File.expand_path('../../tmp/generator_test', __dir__)

      setup do
        prepare_destination
      end

      def test_generates_initializer
        run_generator

        assert_file 'config/initializers/ssr_deno.rb' do |content|
          assert_match(/ssr-deno configuration/, content)
          assert_match(/Rails\.application\.config\.ssr_deno\.bundles/, content)
          assert_match(%r{dist/server/entry-server\.js}, content)
        end
      end

      def test_initializer_contains_default_bundle_path
        run_generator

        assert_file 'config/initializers/ssr_deno.rb' do |content|
          assert_match(%r{application: Rails\.root\.join\('dist/server/entry-server\.js'\)}, content)
        end
      end

      def test_initializer_contains_commented_options
        run_generator

        assert_file 'config/initializers/ssr_deno.rb' do |content|
          assert_match(/#.*ssr_deno\.enabled/, content)
          assert_match(/#.*ssr_deno\.auto_reload/, content)
          assert_match(/#.*ssr_deno\.raise_on_render_error/, content)
        end
      end
    end
  end
end
