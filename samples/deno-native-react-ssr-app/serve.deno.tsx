/// <reference types="@types/deno" />

import { renderToString } from "react-dom/server"

const PORT = parseInt(Deno.env.get("PORT") || "3109", 10)

function App({ name }: { name: string }) {
  return (
    <div id="root">
      <h1>Hello {name}!</h1>
      <p>Rendered with Deno native React SSR — no Vite, no build step.</p>
    </div>
  )
}

Deno.serve({ port: PORT }, (req: Request) => {
  const name = new URL(req.url).searchParams.get("name") || "World"

  try {
    const body = renderToString(<App name={name} />)
    const html = `<!DOCTYPE html>
<html>
  <head><title>Hello ${name}</title></head>
  <body>${body}</body>
</html>`
    return new Response(html, {
      status: 200,
      headers: { "Content-Type": "text/html; charset=utf-8" },
    })
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err)
    console.error("Render error:", message)
    return new Response(`Render error: ${message}`, { status: 500 })
  }
})

console.log(`Deno Native React SSR server at http://localhost:${PORT}`)
console.log(`Try: http://localhost:${PORT}?name=Maurizio`)
