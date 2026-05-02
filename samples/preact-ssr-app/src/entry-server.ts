import { renderToString } from 'preact-render-to-string'
import { h } from 'preact'
import { App } from './App.tsx'

function render(argsJson: string): string {
  const { data } = JSON.parse(argsJson)
  return renderToString(h(App, { data }))
}

// @ts-ignore: globalThis augmentation
globalThis.render = render
