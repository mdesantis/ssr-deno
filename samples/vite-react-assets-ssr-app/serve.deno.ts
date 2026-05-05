/// <reference types="@types/deno" />
import { createRequire } from 'node:module'
import { join, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'

const __dirname = dirname(fileURLToPath(import.meta.url))
const PORT = parseInt(Deno.env.get('PORT') || '3115', 10)

const DIST_CLIENT = join(__dirname, 'dist', 'client')
const DIST_SERVER = join(__dirname, 'dist', 'server')
const MANIFEST_PATH = join(DIST_CLIENT, '.vite', 'manifest.json')
const BUNDLE_PATH = join(DIST_SERVER, 'entry-server.js')

interface ManifestEntry {
  file: string
  src?: string
  css?: string[]
  assets?: string[]
  imports?: string[]
  isEntry?: boolean
}

type Manifest = Record<string, ManifestEntry>

let manifest: Manifest | null = null
let renderFn: ((argsJson: string) => string) | null = null

function loadManifest(): Manifest {
  if (manifest) return manifest

  try {
    const text = Deno.readTextFileSync(MANIFEST_PATH)
    manifest = JSON.parse(text) as Manifest
    console.log(`Manifest loaded with ${Object.keys(manifest).length} entries`)
    return manifest
  } catch (_err) {
    console.warn('No manifest found -- client build not performed')
    return {}
  }
}

function getEntryCssTags(entrySrc: string): string {
  const m = loadManifest()
  const entry = m[entrySrc]
  if (!entry) return ''

  const cssFiles = entry.css ?? []
  return cssFiles
    .map((css) => `    <link rel="stylesheet" href="/${css}">`)
    .join('\n')
}

function getEntryClientJsTag(entrySrc: string): string {
  const m = loadManifest()
  const entry = m[entrySrc]
  if (!entry) return ''

  const jsFile = entry.file
  return `    <script type="module" src="/${jsFile}"></script>`
}

function _collectAllCss(entrySrc: string): string[] {
  const m = loadManifest()
  const entry = m[entrySrc]
  if (!entry) return []

  const cssFiles = new Set<string>(entry.css ?? [])

  for (const importKey of entry.imports ?? []) {
    const imported = m[importKey]
    if (imported?.css) {
      for (const css of imported.css) {
        cssFiles.add(css)
      }
    }
  }

  return Array.from(cssFiles)
}

function _collectAllJs(entrySrc: string): string[] {
  const m = loadManifest()
  const entry = m[entrySrc]
  if (!entry) return []

  const jsFiles = new Set<string>()
  jsFiles.add(entry.file)

  for (const importKey of entry.imports ?? []) {
    const imported = m[importKey]
    if (imported?.file) {
      jsFiles.add(imported.file)
    }
  }

  return Array.from(jsFiles)
}

function serveStaticFile(pathname: string): Response | null {
  const cleanPath = pathname.replace(/^\//, '')
  const filePath = join(DIST_CLIENT, cleanPath)

  if (!filePath.startsWith(DIST_CLIENT)) {
    return new Response('Forbidden', { status: 403 })
  }

  try {
    const stat = Deno.statSync(filePath)
    if (!stat.isFile) return null

    const ext = cleanPath.split('.').pop()?.toLowerCase()
    const contentTypes: Record<string, string> = {
      js: 'application/javascript',
      mjs: 'application/javascript',
      css: 'text/css',
      svg: 'image/svg+xml',
      png: 'image/png',
      jpg: 'image/jpeg',
      jpeg: 'image/jpeg',
      gif: 'image/gif',
      webp: 'image/webp',
      ico: 'image/x-icon',
      woff: 'font/woff',
      woff2: 'font/woff2',
      ttf: 'font/ttf',
    }

    const contentType = contentTypes[ext ?? ''] || 'application/octet-stream'

    return new Response(Deno.readFileSync(filePath), {
      headers: { 'Content-Type': contentType },
    })
  } catch {
    return null
  }
}

async function init() {
  const bundleCode = await Deno.readTextFile(BUNDLE_PATH)
  const scriptCode = bundleCode.replace(/export\s+\{[^}]+\};?\s*$/, '')

  const require = createRequire(BUNDLE_PATH)

  const fn = new Function('require', `
    ${scriptCode}
    return typeof render !== "undefined" ? render : null;
  `)

  renderFn = fn(require) as typeof renderFn
  if (typeof renderFn !== 'function') {
    console.error('Bundle did not export a render function')
    Deno.exit(1)
  }

  const sizeKB = (bundleCode.length / 1024).toFixed(0)
  console.log(`SSR bundle loaded (${sizeKB} KB)`)
  console.log(`Client build: ${DIST_CLIENT}`)

  loadManifest()
}

await init()

Deno.serve({ port: PORT }, (req: Request) => {
  const url = new URL(req.url)
  const pathname = url.pathname

  if (pathname === '/' || pathname === '/index.html') {
    try {
      const name = url.searchParams.get('name') || 'World'
      const title = url.searchParams.get('title') || 'React Assets SSR'

      if (!renderFn) {
        return new Response('SSR runtime not initialized', { status: 500 })
      }

      const result = renderFn(
        JSON.stringify({ data: { name, title } })
      )
      const { html, css: inlineCss } = JSON.parse(result)

      const cssTags = getEntryCssTags('src/entry-client.ts')
      const clientJsTag = getEntryClientJsTag('src/entry-client.ts')

      const fullHtml = `<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>${title}</title>
${inlineCss}
${cssTags}
  </head>
  <body>
    <div id="root">${html}</div>
    <script>window.__SSR_DATA = ${JSON.stringify({ name, title })}</script>
${clientJsTag}
  </body>
</html>`

      return new Response(fullHtml, {
        headers: { 'Content-Type': 'text/html; charset=utf-8' },
      })
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err)
      console.error('Render error:', message)
      return new Response(`Render error: ${message}`, { status: 500 })
    }
  }

  const staticResponse = serveStaticFile(pathname)
  if (staticResponse) return staticResponse

  return new Response('Not Found', { status: 404 })
})

console.log(`Assets SSR server running at http://localhost:${PORT}`)
console.log(`Try: http://localhost:${PORT}?name=Developer&title=My%20App`)
console.log(`Static files served from: ${DIST_CLIENT}`)
