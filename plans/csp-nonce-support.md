# CSP Nonce Support

Document how to pass CSP nonce through SSR data hash. No library code changes needed — nonce already flows through `ssr_render` data.

Extracted from `rails-integration.md` Phase 3 — see that plan for full context.

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

## Scope

1. **Document** in `lib/ssr/deno/rails/helper.rb` and README how to pass nonce via `ssr_render` data hash
2. **(Optional)** Update `samples/react-mui-emotion-ssr-app/` to demonstrate nonce usage with Emotion `<style>` tags

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

---

## Files changed

| File | Change |
|------|--------|
| `lib/ssr/deno/rails/helper.rb` | Add doc comment: nonce via data hash |
| `README.md` | Add CSP nonce usage section |
| `samples/react-mui-emotion-ssr-app/src/entry-server.ts` | (Optional) Add nonce support from data |

---

## Verification

1. CSP nonce works through data hash — no gem code change needed
2. README example correct
3. Optional sample builds and nonce passes through
