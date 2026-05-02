# frozen_string_literal: true

SAMPLES = %w[
  react-ssr-app
  vanilla-ssr-app
  vue-ssr-app
  svelte-ssr-app
  react-mui-emotion-ssr-app
  react-mui-ssr-app
].freeze

desc 'Build all SSR sample bundles'
task 'samples:build' => SAMPLES.map { |s| "samples:build:#{s}" }

SAMPLES.each do |sample|
  desc "Build the #{sample} SSR bundle"
  task "samples:build:#{sample}" do
    sh 'deno', 'task', 'build', chdir: "samples/#{sample}"
  end
end
