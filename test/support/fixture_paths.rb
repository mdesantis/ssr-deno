# frozen_string_literal: true

module TestFixturePaths
  GEM_ROOT = File.expand_path('../..', __dir__)
  FIXTURES_DIR = File.join(GEM_ROOT, 'test', 'fixtures')

  MINIMAL_BUNDLE = File.join(FIXTURES_DIR, 'minimal-bundle.js').freeze
  CHUNKED_BUNDLE = File.join(FIXTURES_DIR, 'chunked-bundle.js').freeze
  LARGE_PAYLOAD_BUNDLE = File.join(FIXTURES_DIR, 'large-payload-bundle.js').freeze
end
