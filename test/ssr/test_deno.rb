# frozen_string_literal: true

require 'test_helper'

module SSR
  class TestDeno < Minitest::Test
    def test_that_it_has_a_version_number
      refute_nil ::SSR::Deno::VERSION
    end

    def test_native_version
      assert_match(/\A\d+\.\d+\.\d+/, ::SSR::Deno.native_version)
    end

    def test_set_max_heap_size_mb
      # May raise JsRuntimeInitializationError if another test already
      # initialized the runtime (OnceLock). We accept either outcome —
      # the purpose is coverage of the accessor and the native method.
      SSR::Deno.max_heap_size_mb = 128
    rescue SSR::Deno::JsRuntimeInitializationError
      # runtime already initialized, that's fine
    end

    def test_set_isolate_pool_size
      # May raise JsRuntimeInitializationError if another test already
      # initialized the runtime (OnceLock). We accept either outcome.
      SSR::Deno.isolate_pool_size = 4
    rescue SSR::Deno::JsRuntimeInitializationError
      # runtime already initialized, that's fine
    end
  end
end
