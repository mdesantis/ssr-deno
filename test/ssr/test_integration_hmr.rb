# frozen_string_literal: true

# HMR (Hot Module Replacement) integration test.
# Verifies that our library picks up Vite SSR bundle rebuilds via auto_reload.
#
# Flow:
#   1. Build the vite-hmr-ssr-app sample
#   2. Load bundle with auto_reload = true
#   3. Render and verify v1 content
#   4. Modify the source entry-server.ts
#   5. Rebuild with Vite
#   6. Render again — verify v2 content (auto_reload detected mtime change)

require 'fileutils'
require 'test_helper'

module SSR
  class TestIntegrationHMR < Minitest::Test
    SAMPLE_DIR = File.expand_path('../../samples/vite-hmr-ssr-app', __dir__)
    SRC_PATH = File.join(SAMPLE_DIR, 'src', 'entry-server.ts')
    BUNDLE_PATH = File.join(SAMPLE_DIR, 'dist', 'server', 'entry-server.js')

    def setup
      skip "#{BUNDLE_PATH} not found — run `bundle exec rake samples:build` first" unless File.exist?(BUNDLE_PATH)
      skip 'deno not on PATH' unless system('deno', '--version', out: File::NULL, err: File::NULL)

      @original_src = File.read(SRC_PATH)
      @original_bundle = File.read(BUNDLE_PATH)
    end

    def teardown
      File.write(SRC_PATH, @original_src) if @original_src
      File.write(BUNDLE_PATH, @original_bundle) if @original_bundle
    end

    def test_hmr_reload_via_vite_rebuild
      bundle = SSR::Deno::Bundle.new(BUNDLE_PATH)
      bundle.auto_reload = true

      html_first = bundle.render({ data: { name: 'HMR' } })

      assert_includes html_first, 'v1'

      File.write(SRC_PATH, <<~TS)
        function render(argsJson: string): string {
          const { name } = JSON.parse(argsJson)
          return `<h1>Hello ${name}!</h1><p>v2</p>`;
        }
        globalThis.render = render
      TS

      build_ok = system('deno', 'task', 'build', chdir: SAMPLE_DIR, out: File::NULL, err: File::NULL)

      assert build_ok, 'Vite build failed on second pass'

      html_second = bundle.render({ data: { name: 'HMR' } })

      assert_includes html_second, 'v2'
      refute_includes html_second, 'v1'
    end
  end
end
