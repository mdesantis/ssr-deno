# frozen_string_literal: true

SAMPLES = %w[
  vite-react-ssr-app
  vite-react-streaming-ssr-app
  vite-ssr-app
  vite-hmr-ssr-app
  vite-vue-ssr-app
  vite-svelte-ssr-app
  vite-react-mui-emotion-ssr-app
  vite-react-mui-ssr-app
  vite-react-emotion-mui-dashboard-ssr-app
  vite-preact-ssr-app
  webpack-ssr-app
  webpack-react-ssr-app
  node-ssr-app
].freeze

desc 'Build all SSR sample bundles'
task 'samples:build' => SAMPLES.map { |s| "samples:build:#{s}" }

SAMPLES.each do |sample|
  desc "Build the #{sample} SSR bundle"
  task "samples:build:#{sample}" do
    sample_dir = File.join(__dir__, '..', 'samples', sample)
    if File.exist?(File.join(sample_dir, 'package.json'))
      sh 'npm', 'run', 'build', chdir: sample_dir
    else
      sh 'deno', 'task', 'build', chdir: sample_dir
    end
  end
end
