# frozen_string_literal: true

module TestFixturePaths
  GEM_ROOT = File.expand_path('../..', __dir__)
  FIXTURES_DIR = File.join(GEM_ROOT, 'test', 'fixtures')

  MINIMAL_BUNDLE = File.join(FIXTURES_DIR, 'minimal-bundle.js').freeze
  CHUNKED_BUNDLE = File.join(FIXTURES_DIR, 'chunked-bundle.js').freeze
  LARGE_PAYLOAD_BUNDLE = File.join(FIXTURES_DIR, 'large-payload-bundle.js').freeze

  ESM_ENTRY = File.join(FIXTURES_DIR, 'esm-entry.js').freeze
  ESM_ADMIN_ENTRY = File.join(FIXTURES_DIR, 'esm-admin-entry.js').freeze
  ESM_NO_RENDER_ENTRY = File.join(FIXTURES_DIR, 'esm-no-render-entry.js').freeze
  ESM_ESCAPE_ENTRY = File.join(FIXTURES_DIR, 'esm-escape-entry.js').freeze
end
