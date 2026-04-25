import { renderToString } from 'react-dom/server'
import { createElement } from 'react'
import App from './App.tsx'

export interface RenderContext {
  component_data: Record<string, unknown>
  props: Record<string, unknown>
}

export function render(url: string, context: RenderContext): string {
  const html = renderToString(
    createElement(App, {
      data: context.component_data,
      extra: context.props,
    })
  )
  return html
}
