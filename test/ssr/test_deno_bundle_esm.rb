# frozen_string_literal: true

require 'test_helper'

module SSR
  # ESM multi-chunk bundle support (experimental).
  # Bundles using ESM `export function render` + local chunk imports are loaded
  # via Deno's module system instead of execute_script. Opt-in with `esm: true`.
  class TestDenoBundleESM < Minitest::Test
    include TestFixturePaths

    def test_esm_bundle_renders_via_named_export
      bundle = SSR::Deno::Bundle.new(ESM_ENTRY, esm: true)
      result = bundle.render({ name: 'World' })

      assert_equal '<h1>World</h1>', result
    end

    def test_esm_bundle_imports_from_chunk
      bundle = SSR::Deno::Bundle.new(ESM_ENTRY, esm: true)
      result = bundle.render({ name: 'Chunk' })

      assert_includes result, 'Chunk'
    end

    def test_script_bundle_still_works_with_esm_false
      bundle = SSR::Deno::Bundle.new(MINIMAL_BUNDLE)
      result = bundle.render({ data: { name: 'Script' } })

      assert_includes result, 'Script'
    end

    def test_two_esm_bundles_coexist_in_same_pool
      app_bundle = SSR::Deno::Bundle.new(ESM_ENTRY, esm: true)
      admin_bundle = SSR::Deno::Bundle.new(ESM_ADMIN_ENTRY, esm: true)

      app_result = app_bundle.render({ name: 'AppUser' })
      admin_result = admin_bundle.render({ name: 'AdminUser' })

      assert_includes app_result, 'AppUser'
      assert_includes admin_result, 'AdminUser'
      assert_includes admin_result, 'admin'
      refute_includes app_result, 'admin'
    end

    def test_esm_bundle_missing_render_export_raises
      assert_raises(SSR::Deno::JsRuntimeInitializationError) do
        SSR::Deno::Bundle.new(ESM_NO_RENDER_ENTRY, esm: true)
      end
    end

    def test_esm_bundle_import_outside_dir_raises
      assert_raises(SSR::Deno::JsRuntimeInitializationError) do
        SSR::Deno::Bundle.new(ESM_ESCAPE_ENTRY, esm: true)
      end
    end

    def test_esm_bundle_reload_works
      bundle = SSR::Deno::Bundle.new(ESM_ENTRY, esm: true)

      bundle.reload

      result = bundle.render({ name: 'Reloaded' })

      assert_includes result, 'Reloaded'
    end
  end
end
