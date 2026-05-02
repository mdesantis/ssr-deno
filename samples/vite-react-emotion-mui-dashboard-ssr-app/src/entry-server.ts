import { renderToString } from 'react-dom/server'
import { createElement } from 'react'
import { CacheProvider } from '@emotion/react'
import createCache from '@emotion/cache'
import createEmotionServer from '@emotion/server/create-instance'
import Dashboard from './Dashboard.tsx'

function render(argsJson: string): string {
  const { data } = JSON.parse(argsJson)

  const cache = createCache({ key: 'dash' })
  const { extractCriticalToChunks, constructStyleTagsFromChunks } = createEmotionServer(cache)

  const html = renderToString(
    createElement(CacheProvider, { value: cache },
      createElement(Dashboard, { data })
    )
  )

  const emotionChunks = extractCriticalToChunks(html)
  const css = constructStyleTagsFromChunks(emotionChunks)

  return JSON.stringify({ html, css })
}

// @ts-ignore: globalThis augmentation
globalThis.render = render
