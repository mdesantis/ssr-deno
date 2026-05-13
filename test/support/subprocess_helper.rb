# frozen_string_literal: true

require 'open3'
require 'rbconfig'
require_relative 'fixture_paths'

module SubprocessHelper
  BOOTSTRAP = <<~RUBY.freeze
    $LOAD_PATH.unshift('#{File.join(TestFixturePaths::GEM_ROOT, 'lib')}')
    require 'ssr/deno'
  RUBY

  def run_subprocess(script, env: {}, bootstrap: BOOTSTRAP)
    full_script = "#{bootstrap}\n#{script}"

    Open3.capture3(env, RbConfig.ruby, '-e', full_script)
  end

  def assert_subprocess(script, msg, env: {})
    _, stderr, status = run_subprocess(script, env: env)

    assert_predicate status, :success?, "#{msg}\nstderr: #{stderr}"
  end
end
