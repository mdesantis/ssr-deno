# frozen_string_literal: true

require 'fileutils'
require 'tmpdir'

module TempBundleHelper
  def setup
    @_temp_dirs = []
    super
  end

  def teardown
    @_temp_dirs&.each { |d| FileUtils.rm_rf(d) }
    super
  end

  def temp_dir(prefix = nil)
    @_temp_dirs ||= []
    dir = Dir.mktmpdir(prefix)

    @_temp_dirs << dir

    dir
  end

  def temp_bundle(js_code, filename: 'bundle.js')
    dir = temp_dir
    path = File.join(dir, filename)
    File.write(path, js_code)

    SSR::Deno::Bundle.new(path)
  end
end
