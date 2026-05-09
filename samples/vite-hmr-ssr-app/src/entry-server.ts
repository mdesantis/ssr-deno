function render(argsJson: string): string {
  const { name } = JSON.parse(argsJson)
  return `<!DOCTYPE html>
<html>
  <head><meta charset="utf-8"><title>HMR ${name}</title></head>
  <body>
    <div id="root">
      <h1>Hello ${name}!</h1>
      <p>v1</p>
    </div>
  </body>
</html>`
}

// @ts-ignore: globalThis augmentation
globalThis.render = render
