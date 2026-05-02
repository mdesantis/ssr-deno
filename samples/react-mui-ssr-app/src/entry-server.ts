import { renderToString } from 'react-dom/server'
import { createElement } from 'react'
import { CacheProvider } from '@emotion/react'
import createCache from '@emotion/cache'
import App from './App.tsx'

function render(argsJson: string): string {
  const { data } = JSON.parse(argsJson)

  const doc = globalThis as Record<string, unknown>
  if (typeof doc.document === 'undefined') {
    const head: Record<string, unknown> = { appendChild: () => { } }
    const el = () => ({ appendChild: () => { }, setAttribute: () => { }, style: {}, addEventListener: () => { }, removeEventListener: () => { } })
    doc.document = { head, createElement: el, querySelectorAll: () => [], querySelector: () => null, createTextNode: () => ({}) }
  }

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
