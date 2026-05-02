/** @jsxImportSource react */
import { renderToString } from 'react-dom/server'
import { App } from './App.tsx'

function render(argsJson: string): string {
  const { data } = JSON.parse(argsJson)
  const name = data?.name ?? 'World'
  const html = renderToString(<App data={data} />)
  return `<!DOCTYPE html>
<html>
  <head><meta charset="utf-8"><title>Hello ${name}</title></head>
  <body><div id="root">${html}</div></body>
</html>`
}

// @ts-ignore: globalThis augmentation
globalThis.render = render
