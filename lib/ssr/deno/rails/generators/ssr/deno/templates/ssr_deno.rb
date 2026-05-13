# frozen_string_literal: true

# ssr-deno configuration
#
# See https://github.com/mdesantis/ssr-deno for documentation.

Rails.application.config.ssr_deno.bundles = {
  application: Rails.root.join('dist/server/entry-server.js')
}

# Set to false to disable SSR entirely.
# Rails.application.config.ssr_deno.enabled = true

# Auto-reload bundles in development when the file changes on disk.
# Rails.application.config.ssr_deno.auto_reload = Rails.env.development?

# Raise on render errors (recommended: true in dev/test, false in production).
# Rails.application.config.ssr_deno.raise_on_render_error = !Rails.env.production?

# Render timeout in milliseconds (default: 500ms, min 100, max 300000).
# Rails.application.config.ssr_deno.render_timeout_ms = 1000

# Enable Node.js built-in module support. Required for SSR bundles that
# depend on @emotion/server or other packages calling require() for
# Node.js built-in modules (stream, buffer, events, ...).
# Adds ~50ms to worker initialization time.
# Rails.application.config.ssr_deno.node_builtins_enabled = false

# Raise on bundle not found (recommended: true in dev/test, false in production).
# Rails.application.config.ssr_deno.raise_on_bundle_error = !Rails.env.production?

# Resolve V8 stack traces to original .tsx source files (default: true in
# development/test, false in production). Requires .js.map sidecars next to
# bundles. Best-effort — silently skips missing or corrupt .map files.
# Rails.application.config.ssr_deno.source_maps_enabled = !Rails.env.production?
