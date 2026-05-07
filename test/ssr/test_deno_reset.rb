# frozen_string_literal: true

require 'test_helper'
require 'support/subprocess_helper'

module SSR
  class TestDenoReset < Minitest::Test
    include TestFixturePaths
    include SubprocessHelper

    def test_render_deadlocks_in_forked_child_without_reset
      bundle_path = TestFixturePaths::MINIMAL_BUNDLE

      script = "bundle_path = '#{bundle_path}'\n"
      script << <<~'RUBY'
        bundle = SSR::Deno::Bundle.new(bundle_path)

        r, w = IO.pipe
        pid = Process.fork do
          r.close
          begin
            bundle.render(nil)
            w.write('unexpected'); w.close; exit!(0)
          rescue => e
            w.write("err:#{e.class}"); w.close; exit!(1)
          end
        end
        w.close

        readable = IO.select([r], nil, nil, 3)
        if readable.nil?
          Process.kill(:KILL, pid)
          Process.waitpid(pid)
          exit 0
        else
          output = r.read
          _, status = Process.waitpid2(pid)
          exit 1
        end
      RUBY

      assert_subprocess(script, 'Expected forked child to deadlock without reset!')
    end

    def test_render_succeeds_in_forked_child_after_reset
      bundle_path = TestFixturePaths::MINIMAL_BUNDLE

      script = "bundle_path = '#{bundle_path}'\n"
      script << <<~'RUBY'
        bundle = SSR::Deno::Bundle.new(bundle_path)

        r, w = IO.pipe
        pid = Process.fork do
          r.close
          SSR::Deno.reset!
          result = bundle.render(nil)
          w.write(result.nil? ? 'nil' : 'ok')
          w.close
          exit!(0)
        end
        w.close
        output = r.read; r.close
        Process.waitpid(pid)
        raise "Render failed in forked child: #{output.inspect}" unless output == 'ok'
        exit 0
      RUBY

      assert_subprocess(script, 'Expected render to succeed in forked worker after SSR::Deno.reset!')
    end

    def test_config_setters_available_after_reset
      bundle_path = TestFixturePaths::MINIMAL_BUNDLE

      script = "bundle_path = '#{bundle_path}'\n"
      script << <<~RUBY
        SSR::Deno::Bundle.new(bundle_path)
        SSR::Deno.reset!
        SSR::Deno.isolate_pool_size = 2
        exit 0
      RUBY

      assert_subprocess(script, 'Expected config setters to work after reset!')
    end

    def test_pool_generation_increments_on_reset
      bundle_path = TestFixturePaths::MINIMAL_BUNDLE

      script = "bundle_path = '#{bundle_path}'\n"
      script << <<~RUBY
        SSR::Deno::Bundle.new(bundle_path)
        gen_before = SSR::Deno.native_pool_generation
        SSR::Deno.reset!
        gen_after = SSR::Deno.native_pool_generation
        raise "Expected generation to increment" unless gen_after == gen_before + 1
        exit 0
      RUBY

      assert_subprocess(script, 'Expected pool generation to increment after reset!')
    end
  end
end
