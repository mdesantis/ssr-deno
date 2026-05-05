import { renderToString } from 'react-dom/server'
import { createElement } from 'react'
import App from './App.tsx'

function render(argsJson: string): string {
  const { data } = JSON.parse(argsJson)

  const html = renderToString(createElement(App, { data }))

  return JSON.stringify({ html, css: '' })
}

// @ts-ignore: globalThis augmentation
globalThis.render = render
