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

desc 'Remove all built SSR sample bundles (forces a full rebuild)'
task 'samples:clean' do
  SAMPLES.each do |sample|
    FileUtils.rm_rf(File.join(__dir__, '..', 'samples', sample, 'dist'))
  end
end

# Source globs that, if newer than the built bundle, mean it's stale and
# needs rebuilding. Covers both the Deno (deno.json/deno.lock) and npm
# (package.json/package-lock.json) sample layouts.
SAMPLE_SOURCE_GLOBS = %w[
  src/**/*
  vite.config.*
  webpack.config.*
  deno.json
  deno.lock
  package.json
  package-lock.json
].freeze

def sample_bundle_stale?(sample_dir, bundle)
  return true unless File.exist?(bundle)

  bundle_mtime = File.mtime(bundle)
  sources = SAMPLE_SOURCE_GLOBS.flat_map { |g| Dir.glob(File.join(sample_dir, g)) }.select { |f| File.file?(f) }

  sources.any? { |f| File.mtime(f) > bundle_mtime }
end

SAMPLES.each do |sample|
  desc "Build the #{sample} SSR bundle"
  task "samples:build:#{sample}" do
    sample_dir = File.join(__dir__, '..', 'samples', sample)
    bundle = File.join(sample_dir, 'dist/server/entry-server.js')

    next unless sample_bundle_stale?(sample_dir, bundle)

    if File.exist?(File.join(sample_dir, 'package.json'))
      sh 'npm', 'install', chdir: sample_dir
      sh 'npm', 'run', 'build', chdir: sample_dir
    else
      sh 'deno', 'task', 'build', chdir: sample_dir
    end
  end
end
