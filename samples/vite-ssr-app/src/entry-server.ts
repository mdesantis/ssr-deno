import { renderToString } from 'react-dom/server'
import { createElement } from 'react'
import App from './App.tsx'

export interface RenderContext {
  component_data: Record<string, unknown>
  props: Record<string, unknown>
  url: string
}

function render(argsJson: string): string {
  const context: RenderContext = JSON.parse(argsJson)
  const html = renderToString(
    createElement(App, {
      data: context.component_data,
      extra: context.props,
    })
  )
  return html
}

// Assign to globalThis so the function is accessible from the embedded V8 isolate
// when the bundle is evaluated via execute_script (not as an ES module).
// The Rust extension looks for globalThis.render to call it.
// @ts-ignore: globalThis augmentation
globalThis.render = render
