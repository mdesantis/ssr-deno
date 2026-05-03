# frozen_string_literal: true

require 'test_helper'
require 'open3'
require 'rbconfig'

module SSR
  class TestDenoStability < Minitest::Test
    MINIMAL_BUNDLE = File.expand_path('../fixtures/minimal-bundle.js', __dir__)
    LARGE_PAYLOAD_BUNDLE = File.expand_path('../fixtures/large-payload-bundle.js', __dir__)
    GEM_ROOT = File.expand_path('../..', __dir__)

    BOOTSTRAP = <<~RUBY.freeze
      require 'tmpdir'
      $LOAD_PATH.unshift('#{File.join(GEM_ROOT, 'lib')}')
      require 'ssr/deno'
    RUBY

    def setup
      @bundle = SSR::Deno::Bundle.new(MINIMAL_BUNDLE)
    end

    def test_no_internal_leaks_over_repeated_renders
      baseline = SSR::Deno.heap_stats['used_heap_size']
      100.times { @bundle.render({ data: { name: 'stress' } }) }
      final = SSR::Deno.heap_stats['used_heap_size']

      assert_operator final, :<, baseline * 3,
                      "Heap grew #{final / baseline}x — possible leak"
    end

    def test_large_data_payload_does_not_crash
      large = { items: Array.new(1000) { { name: 'x' * 80, value: rand } } }
      bundle = SSR::Deno::Bundle.new(LARGE_PAYLOAD_BUNDLE)
      result = bundle.render({ data: large })

      assert_match(/<div>/, result)
    end

    def test_edge_case_data_does_not_crash
      @bundle.render({})
      @bundle.render({ data: nil })
      @bundle.render({ data: { deep: { deeper: { deepest: [1, 2, 3] } } } })
    end

    def test_rapid_reload_does_not_crash
      bundle_path = File.expand_path('../fixtures/minimal-bundle.js', __dir__)
      20.times do
        bundle = SSR::Deno::Bundle.new(bundle_path)
        3.times do
          bundle.reload
          bundle.render({ data: { name: 'reload' } })
        end
      end
    end

    def test_oom_produces_out_of_memory_error
      assert_subprocess(<<~RUBY, 'Expected SSR::Deno::JsRuntimeOutOfMemoryError on OOM')
        SSR::Deno.max_heap_size_mb = 16
        SSR::Deno.isolate_pool_size = 1
        begin
          Dir.mktmpdir do |dir|
            bundle_path = File.join(dir, 'leak-bundle.js')
            File.write(bundle_path, <<~JS)
              var leak = [];
              globalThis.render = function() {
                leak.push(new Array(64000).fill('x'));
                return '<p>ok</p>';
              };
            JS
            bundle = SSR::Deno::Bundle.new(bundle_path)
            100.times { bundle.render({}) }
          end
          exit 1
        rescue SSR::Deno::JsRuntimeOutOfMemoryError
          exit 0
        end
      RUBY
    end

    private

    def assert_subprocess(body, msg)
      script = "#{BOOTSTRAP}#{body}"
      _, _, status = Open3.capture3(RbConfig.ruby, '-e', script, chdir: GEM_ROOT)

      assert_predicate status.exitstatus, :zero?, msg
    end
  end
end
