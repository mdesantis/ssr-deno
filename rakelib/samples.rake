# frozen_string_literal: true

SAMPLES = %w[
  vite-react-ssr-app
  vite-ssr-app
  vite-vue-ssr-app
  vite-svelte-ssr-app
  vite-react-mui-emotion-ssr-app
  vite-react-mui-ssr-app
  vite-react-emotion-mui-dashboard-ssr-app
  vite-preact-ssr-app
].freeze

desc 'Build all SSR sample bundles'
task 'samples:build' => SAMPLES.map { |s| "samples:build:#{s}" }

SAMPLES.each do |sample|
  desc "Build the #{sample} SSR bundle"
  task "samples:build:#{sample}" do
    sh 'deno', 'task', 'build', chdir: "samples/#{sample}"
  end
end
