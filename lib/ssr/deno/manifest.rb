# frozen_string_literal: true

require 'json'

module SSR
  module Deno
    # Parses Vite's build manifest (.vite/manifest.json) to discover
    # client-side assets (JS, CSS, static files) for injection into
    # server-rendered HTML.
    #
    # The manifest maps source entry points to their hashed output files
    # and lists all CSS, JS, and static assets referenced by each entry.
    #
    # Example manifest structure:
    #   {
    #     "src/entry-client.ts": {
    #       "file": "assets/entry-client-abc123.js",
    #       "css": ["assets/index-def456.css"],
    #       "assets": ["assets/logo-ghi789.svg"],
    #       "imports": ["_shared-jkl012.js"]
    #     },
    #     "_shared-jkl012.js": {
    #       "file": "assets/_shared-jkl012.js",
    #       "css": ["assets/vendor-mno345.css"]
    #     }
    #   }
    #
    # Usage:
    #   manifest = SSR::Deno::Manifest.new('dist/client/.vite/manifest.json')
    #   manifest.css_tags('src/entry-client.ts')  # => "<link ...>"
    #   manifest.client_js_tag('src/entry-client.ts')  # => "<script ...>"
    #
    class Manifest
      # @return [Hash<String, Hash>] raw manifest data
      attr_reader :data

      # @param manifest_path [String] Path to .vite/manifest.json
      # @raise [ArgumentError] if file does not exist
      def initialize(manifest_path)
        @manifest_path = manifest_path.to_s
        raise ArgumentError, "Manifest not found: #{@manifest_path}" unless File.exist?(@manifest_path)

        @data = JSON.parse(File.read(@manifest_path))
      end

      # Returns all CSS files (including transitive imports) for a source entry.
      #
      # @param source [String] Source entry key (e.g., "src/entry-client.ts")
      # @return [Array<String>] CSS file paths relative to dist/client/
      def css_files(source)
        entry = @data[source]
        return [] unless entry

        collect_from_entry(entry, 'css')
      end

      # Returns all JS files (including transitive imports) for a source entry.
      # The main entry file is always first, followed by imported chunks.
      #
      # @param source [String] Source entry key
      # @return [Array<String>] JS file paths relative to dist/client/
      def js_files(source)
        entry = @data[source]
        return [] unless entry

        js = [entry['file']]

        entry['imports']&.each do |import_key|
          imported = @data[import_key]
          js << imported['file'] if imported && imported['file']
        end

        js
      end

      # Returns all static asset files (images, fonts, etc.) referenced
      # by a source entry and its transitive imports.
      #
      # @param source [String] Source entry key
      # @return [Array<String>] Asset file paths relative to dist/client/
      def asset_files(source)
        entry = @data[source]
        return [] unless entry

        collect_from_entry(entry, 'assets')
      end

      # Returns HTML `<link>` tags for all CSS files of a source entry.
      #
      # @param source [String] Source entry key
      # @param prefix [String] URL prefix for asset paths (default: "/")
      # @return [String] HTML string with newline-separated `<link>` tags
      def css_tags(source, prefix: '/')
        css_files(source).map { |f| "    <link rel=\"stylesheet\" href=\"#{prefix}#{f}\">" }.join("\n")
      end

      # Returns an HTML `<script>` tag for the main JS file of a source entry.
      # Only the main entry file is included -- import chunks are loaded
      # dynamically by the module loader.
      #
      # @param source [String] Source entry key
      # @param prefix [String] URL prefix for asset paths (default: "/")
      # @return [String] HTML `<script type="module">` tag
      def client_js_tag(source, prefix: '/')
        entry = @data[source]
        return '' unless entry && entry['file']

        "    <script type=\"module\" src=\"#{prefix}#{entry['file']}\"></script>"
      end

      # Returns all `<script>` tags for JS files (main entry + import chunks).
      # Use this only if you need explicit script tags instead of ESM imports.
      #
      # @param source [String] Source entry key
      # @param prefix [String] URL prefix for asset paths (default: "/")
      # @return [String] HTML string with newline-separated `<script>` tags
      def all_js_tags(source, prefix: '/')
        js_files(source).map { |f| "    <script type=\"module\" src=\"#{prefix}#{f}\"></script>" }.join("\n")
      end

      # Returns the public URL prefix for static assets served from
      # the client build output directory.
      #
      # @param source [String] Source entry key
      # @return [Array<String>] URL paths for all static assets
      def asset_urls(source, prefix: '/')
        asset_files(source).map { |f| "#{prefix}#{f}" }
      end

      # Returns a hash of all discovered assets for a source entry.
      # Useful for passing asset info to views or templates.
      #
      # @param source [String] Source entry key
      # @param prefix [String] URL prefix for asset paths (default: "/")
      # @return [Hash] Asset info with keys: :css_tags, :client_js_tag, :asset_urls
      def assets(source, prefix: '/')
        {
          css_tags: css_tags(source, prefix:),
          client_js_tag: client_js_tag(source, prefix:),
          asset_urls: asset_urls(source, prefix:)
        }
      end

      private

      def collect_from_entry(entry, field)
        result = Set.new(entry[field] || [])

        entry['imports']&.each do |import_key|
          imported = @data[import_key]
          imported[field]&.each { |item| result << item } if imported
        end

        result.to_a
      end
    end
  end
end
