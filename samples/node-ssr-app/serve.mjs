import { createServer } from 'node:http'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'

const __dirname = dirname(fileURLToPath(import.meta.url))
const PORT = parseInt(process.env.PORT || '3113', 10)
const BUNDLE_PATH = join(__dirname, 'dist/server/entry-server.js')

const bundleCode = readFileSync(BUNDLE_PATH, 'utf-8')

const fn = new Function(`
  ${bundleCode}
  return typeof render !== "undefined" ? render : null
`)

const renderFn = fn()
if (typeof renderFn !== 'function') {
  console.error('Bundle did not export a render function')
  process.exit(1)
}

const sizeKB = (bundleCode.length / 1024).toFixed(0)
console.log(`Bundle loaded successfully (${sizeKB} KB)`)

const server = createServer((req, res) => {
  const url = new URL(req.url, `http://localhost:${PORT}`)
  const name = url.searchParams.get('name') || 'World'

  try {
    const html = renderFn(JSON.stringify({ name }))
    res.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8' })
    res.end(html)
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err)
    console.error('Render error:', message)
    res.writeHead(500)
    res.end(`Render error: ${message}`)
  }
})

server.listen(PORT, () => {
  console.log(`Vanilla SSR test server running at http://localhost:${PORT}`)
  console.log(`Try: http://localhost:${PORT}?name=Developer`)
})
