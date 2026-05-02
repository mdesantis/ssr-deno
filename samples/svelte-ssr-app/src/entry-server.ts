import { render as renderSvelte } from 'svelte/server'
import App from './App.svelte'

function render(argsJson: string): string {
  const { data } = JSON.parse(argsJson)
  const name = (data?.name as string) ?? 'World'
  const result = renderSvelte(App, { props: { data } })
  return `<!DOCTYPE html>
<html>
  <head>${result.head}<title>Hello ${name}</title></head>
  <body><div id="root">${result.body}</div></body>
</html>`
}

// @ts-ignore: globalThis augmentation
globalThis.render = render
