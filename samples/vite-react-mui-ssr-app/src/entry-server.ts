import { renderToString } from 'react-dom/server'
import { createElement } from 'react'
import { CacheProvider } from '@emotion/react'
import createCache from '@emotion/cache'
import App from './App.tsx'

function render(argsJson: string): string {
  const { data } = JSON.parse(argsJson)

  const cache = createCache({ key: 'mui' })
  const html = renderToString(
    createElement(CacheProvider, { value: cache },
      createElement(App, { data })
    )
  )

  return html
}

// @ts-ignore: globalThis augmentation
globalThis.render = render
