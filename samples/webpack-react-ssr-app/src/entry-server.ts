import { renderToString } from 'react-dom/server'
import { createElement } from 'react'
import App from './App'

export interface RenderContext {
  data: Record<string, unknown>
}

function render(argsJson: string): string {
  const context: RenderContext = JSON.parse(argsJson)
  const html = renderToString(
    createElement(App, {
      data: context.data
    })
  )
  return html
}

// @ts-ignore: globalThis augmentation
globalThis.render = render
