# frozen_string_literal: true

desc 'Check performance regression (via test:perf)'
task 'perf:check' => 'test:perf'
