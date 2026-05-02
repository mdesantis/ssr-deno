/// <reference types="@types/deno" />

const PORT = parseInt(Deno.env.get("PORT") || "3101", 10)

function render(name: string): string {
  return `<!DOCTYPE html>
<html>
  <head><meta charset="utf-8"><title>Hello ${name}</title></head>
  <body>
    <div id="root">
      <h1>Hello ${name}!</h1>
      <p>Rendered with Deno native SSR — no build step, no framework.</p>
    </div>
  </body>
</html>`
}

Deno.serve({ port: PORT }, (req: Request) => {
  const name = new URL(req.url).searchParams.get("name") || "World"

  try {
    return new Response(render(name), {
      status: 200,
      headers: { "Content-Type": "text/html; charset=utf-8" },
    })
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err)
    console.error("Render error:", message)
    return new Response(`Render error: ${message}`, { status: 500 })
  }
})

console.log(`Deno Native SSR server at http://localhost:${PORT}`)
console.log(`Try: http://localhost:${PORT}?name=Developer`)
