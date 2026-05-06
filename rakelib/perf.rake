# frozen_string_literal: true

desc 'Check performance regression (invariants + baseline comparison)'
task 'perf:check' do
  ruby 'bench/perf-check.rb'
end

desc 'Update performance baselines in config/perf-baselines.yml'
task 'perf:baseline:update' do
  ruby 'bench/perf-check.rb', '--update-baseline'
end
