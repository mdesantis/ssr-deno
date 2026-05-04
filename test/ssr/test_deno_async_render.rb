# frozen_string_literal: true

require 'test_helper'

module SSR
  class TestDenoAsyncRender < Minitest::Test
    MINIMAL_BUNDLE  = File.expand_path('../fixtures/minimal-bundle.js', __dir__)
    ASYNC_IMMEDIATE = File.expand_path('../fixtures/async-immediate-bundle.js', __dir__)
    ASYNC_RESOLVE   = File.expand_path('../fixtures/async-resolve-bundle.js', __dir__)
    ASYNC_REJECT    = File.expand_path('../fixtures/async-reject-bundle.js', __dir__)
    ASYNC_CHAINED   = File.expand_path('../fixtures/async-chained-bundle.js', __dir__)
    ASYNC_HANG      = File.expand_path('../fixtures/async-hang-bundle.js', __dir__)

    def test_sync_render_still_works
      bundle = SSR::Deno::Bundle.new(MINIMAL_BUNDLE)
      html = bundle.render({ data: { name: 'Sync' } })

      assert_includes html, 'Sync'
    end

    def test_async_function_resolves
      bundle = SSR::Deno::Bundle.new(ASYNC_IMMEDIATE)
      html = bundle.render({})

      assert_includes html, 'async-immediate'
    end

    def test_promise_resolve_settled
      bundle = SSR::Deno::Bundle.new(ASYNC_RESOLVE)
      html = bundle.render({})

      assert_includes html, 'async-resolve'
    end

    def test_async_reject_raises_render_error
      bundle = SSR::Deno::Bundle.new(ASYNC_REJECT)

      assert_raises(SSR::Deno::RenderError) do
        bundle.render({})
      end
    end

    def test_chained_promise_resolves
      bundle = SSR::Deno::Bundle.new(ASYNC_CHAINED)
      html = bundle.render({})

      assert_includes html, 'async-chained'
    end

    def test_promise_never_settles_raises_render_error
      bundle = SSR::Deno::Bundle.new(ASYNC_HANG)
      error = assert_raises(SSR::Deno::RenderError) do
        bundle.render({})
      end

      assert_includes error.message, 'Render timed out'
    end
  end
end
