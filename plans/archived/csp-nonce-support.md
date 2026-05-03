# CSP Nonce Support

**Completed.** Documented in [`docs/csp-nonce.md`](docs/csp-nonce.md) — linked from README.

No library code changes needed — nonce flows through `ssr_render` data hash to JS bundle.

Original scope extracted from `rails-integration.md` Phase 3.

---

## Current capability

Nonce already passable via JS `render` data hash. User controls propagation entirely from JS side:

```ruby
# Rails view — pass nonce through data hash
<%= ssr_render({ page: "home", nonce: content_security_policy_nonce }) %>
```

```ts
// entry-server.ts — JS side reads nonce from data
function render(argsJson: string): string {
  const { data, nonce } = JSON.parse(argsJson)
  // use nonce for inline <script>/<style> tags
}
```

No gem-side changes required.

---

## Emotion nonce example

Emotion's `createCache` accepts a `nonce` option. JS entry reads nonce from data:

```ts
import { renderToString } from 'react-dom/server'
import { createElement } from 'react'
import { CacheProvider } from '@emotion/react'
import createCache from '@emotion/cache'
import createEmotionServer from '@emotion/server/create-instance'
import App from './App.tsx'

function render(argsJson: string): string {
  const { data, nonce } = JSON.parse(argsJson)

  const cache = createCache({
    key: 'ssr',
    nonce, // ← propagate nonce to inline <style> tags
  })
  const { extractCriticalToChunks, constructStyleTagsFromChunks } = createEmotionServer(cache)

  const html = renderToString(
    createElement(CacheProvider, { value: cache },
      createElement(App, { data })
    )
  )

  const emotionChunks = extractCriticalToChunks(html)
  const css = constructStyleTagsFromChunks(emotionChunks)  // ← tags include nonce

  return JSON.stringify({ html, css })
}

globalThis.render = render
```

See `samples/vite-react-mui-emotion-ssr-app/` for working example.
