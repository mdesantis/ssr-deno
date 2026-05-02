import { createSSRApp } from 'vue'
import { renderToString } from 'vue/server-renderer'
import App from './App.vue'

async function render(argsJson: string): Promise<string> {
  const { data } = JSON.parse(argsJson)
  const name = (data?.name as string) ?? 'World'
  const app = createSSRApp(App, { data })
  const body = await renderToString(app)
  return `<!DOCTYPE html>
<html>
  <head><meta charset="utf-8"><title>Hello ${name}</title></head>
  <body><div id="root">${body}</div></body>
</html>`
}

// @ts-ignore: globalThis augmentation
globalThis.render = render
