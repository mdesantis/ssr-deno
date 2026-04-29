# frozen_string_literal: true

def rbs_prototype_subtract
  require 'open3'
  require 'tempfile'
  sources = FileList['lib/**/*.rb'].to_a
  Tempfile.create(['prototype', '.rbs']) do |f|
    system("rbs prototype rb #{sources.join(' ')} > #{f.path}")
    out, = Open3.capture2('rbs', 'subtract', f.path, 'sig/ssr/deno.rbs')
    return out
  end
end

namespace :rbs do
  desc 'Validate RBS signatures'
  task :validate do
    sh 'rbs', '-Isig', 'validate'
  end

  desc 'Fail if sig/ is missing declarations found in Ruby source'
  task :up_to_date do
    out = rbs_prototype_subtract
    next if out.strip.empty?

    warn out
    abort 'rbs:up_to_date: sig/ is incomplete — run `rake rbs:diff` to see drift'
  end

  desc 'Print declarations found in source but missing from sig/'
  task :diff do
    print rbs_prototype_subtract
  end
end

desc 'Validate RBS signatures and check sig/ is up to date'
task rbs: %w[rbs:validate rbs:up_to_date]
