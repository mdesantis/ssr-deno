# CSP Nonce Support

Nonce passable via `ssr_render` data hash. No library code changes needed — nonce flows through render data to the JS bundle.

## Usage

Pass the nonce through the data hash in your Rails view:

```erb
<%= ssr_render({ page: "home", nonce: content_security_policy_nonce }) %>
```

In your JS entry-server, read the nonce from the parsed data:

```ts
function render(argsJson: string): string {
  const { data, nonce } = JSON.parse(argsJson)
  // use nonce for inline <script>/<style> tags
}
globalThis.render = render
```

## Emotion Example

Emotion's `createCache` accepts a `nonce` option. The JS entry reads nonce from data:

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

## Samples

See `samples/vite-react-mui-emotion-ssr-app/` for a working Emotion + nonce setup.
