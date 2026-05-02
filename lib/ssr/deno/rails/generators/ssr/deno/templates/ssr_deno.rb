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

# Enable Node.js built-in module support. Required for SSR bundles that
# depend on @emotion/server or other packages calling require() for
# Node.js built-in modules (stream, buffer, events, …).
# Adds ~50ms to worker initialization time.
# Rails.application.config.ssr_deno.node_builtins_enabled = false
