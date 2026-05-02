import { renderToString } from 'react-dom/server'
import { createElement } from 'react'
import { CacheProvider } from '@emotion/react'
import createCache from '@emotion/cache'
import App from './App'

function render(argsJson: string): string {
  const { data } = JSON.parse(argsJson)

  // Emotion's createCache references document.head at call time.
  // Provide a minimal SSR-compatible document stub.
  const doc = globalThis as Record<string, unknown>
  if (typeof doc.document === 'undefined') {
    const head: Record<string, unknown> = { appendChild: () => {} }
    const el = () => ({ appendChild: () => {}, setAttribute: () => {}, style: {}, addEventListener: () => {}, removeEventListener: () => {} })
    doc.document = { head, createElement: el, querySelectorAll: () => [], querySelector: () => null, createTextNode: () => ({}) }
  }

  const cache = createCache({ key: 'ssr' })

  const html = renderToString(
    createElement(CacheProvider, { value: cache },
      createElement(App, { data })
    )
  )

  const styles = extractEmotionStyles(cache)
  const css = `<style data-emotion="ssr">${styles}</style>`

  return JSON.stringify({ html, css })
}

function extractEmotionStyles(cache: ReturnType<typeof createCache>): string {
  const inserted = (cache as unknown as { inserted: Record<string, string | true> }).inserted
  const styles: string[] = []

  for (const id of Object.keys(inserted)) {
    const style = inserted[id]
    if (typeof style === 'string') {
      styles.push(style)
    }
  }

  return styles.join('')
}

// @ts-ignore: globalThis augmentation
globalThis.render = render
