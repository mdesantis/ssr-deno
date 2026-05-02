function render(argsJson: string): string {
  const { name } = JSON.parse(argsJson)
  return `<!DOCTYPE html>
<html>
  <head><meta charset="utf-8"><title>Hello ${name}</title></head>
  <body>
    <div id="root">
      <h1>Hello ${name}!</h1>
      <p>Rendered with TypeScript + Webpack SSR.</p>
    </div>
  </body>
</html>`
}

globalThis.render = render
