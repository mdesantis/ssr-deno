# New Sample Apps — Plan

Add 5 new SSR sample apps under `samples/` following [`samples/vite-react-ssr-app/`](../samples/vite-react-ssr-app/) pattern.

---

## Contract (all samples)

1. `deno task build` → `dist/server/entry-server.js` (self-contained, `noExternal: true`)
2. `globalThis.render(argsJson: string): string` — Ruby calls this via V8
3. `deno task serve` — manual test server (same [`serve.deno.ts`](../samples/vite-react-ssr-app/serve.deno.ts) pattern)
4. Works with `SSR::Deno::Bundle.new(path)` + `bundle.render(data)`

**Pattern:**

```
samples/{name}/
├── deno.json          # tasks: build, serve; npm imports
├── serve.deno.ts      # Deno HTTP server (copy from vite-react-ssr-app)
├── tsconfig.json      # TypeScript config
├── vite.config.ts     # Vite SSR config
└── src/
    ├── entry-server.ts   # globalThis.render = fn
    ├── App.{tsx|vue|svelte}
    └── components/
```

---

## Prerequisite: Async Render Support in Rust

See separate plan: [`plans/archived/async-render-support.md`](archived/async-render-support.md)

Vue SSR `renderToString` returns a `Promise`. Current [`call_render`](../ext/ssr_deno/src/deno_runtime_wrapper/call_render.rs) only handles sync functions.

**Required before Vue sample can work.**

---

## Sample 1: Vanilla SSR (`samples/vite-ssr-app/`)

**Purpose:** Baseline. No framework. Pure TS template literals.

**Deps:** `vite` only.

**Key entry:**

```ts
function render(argsJson: string): string {
  const { name } = JSON.parse(argsJson)
  return `<!DOCTYPE html>
<html>
  <head><title>Hello ${name}</title></head>
  <body><div id="root"><h1>Hello ${name}!</h1></div></body>
</html>`
}
globalThis.render = render
```

**Files to create:**

| File | Content |
|------|---------|
| [`samples/vite-ssr-app/deno.json`](../samples/vite-ssr-app/deno.json) | imports: vite; tasks: build, serve |
| [`samples/vite-ssr-app/vite.config.ts`](../samples/vite-ssr-app/vite.config.ts) | no plugins; ssr.target webworker |
| [`samples/vite-ssr-app/tsconfig.json`](../samples/vite-ssr-app/tsconfig.json) | same as existing |
| [`samples/vite-ssr-app/serve.deno.ts`](../samples/vite-ssr-app/serve.deno.ts) | same as existing |
| [`samples/vite-ssr-app/src/entry-server.ts`](../samples/vite-ssr-app/src/entry-server.ts) | sync render fn |

---

## Sample 2: Vue SSR (`samples/vite-vue-ssr-app/`)

**Purpose:** Vue 3 SSR with SFC.

**Deps:** `vue`, `@vue/server-renderer`, `@vitejs/plugin-vue`, `vite`

**Requires:** Async render prerequisite (above).

**Key entry:**

```ts
import { createSSRApp } from 'vue'
import { renderToString } from 'vue/server-renderer'
import App from './App.vue'

async function render(argsJson: string): Promise<string> {
  const { data } = JSON.parse(argsJson)
  const app = createSSRApp(App, { data })
  return await renderToString(app)
}
globalThis.render = render
```

**Files to create:**

| File | Content |
|------|---------|
| [`samples/vite-vue-ssr-app/deno.json`](../samples/vite-vue-ssr-app/deno.json) | vue/vue-ssr/vite-plugin-vue imports |
| [`samples/vite-vue-ssr-app/vite.config.ts`](../samples/vite-vue-ssr-app/vite.config.ts) | plugin: vue() |
| [`samples/vite-vue-ssr-app/tsconfig.json`](../samples/vite-vue-ssr-app/tsconfig.json) | same |
| [`samples/vite-vue-ssr-app/serve.deno.ts`](../samples/vite-vue-ssr-app/serve.deno.ts) | same pattern |
| [`samples/vite-vue-ssr-app/src/entry-server.ts`](../samples/vite-vue-ssr-app/src/entry-server.ts) | async render, await renderToString |
| [`samples/vite-vue-ssr-app/src/App.vue`](../samples/vite-vue-ssr-app/src/App.vue) | Vue SFC with full HTML doc |
| [`samples/vite-vue-ssr-app/src/components/HelloWorld.vue`](../samples/vite-vue-ssr-app/src/components/HelloWorld.vue) | child component |

**Note:** Vue SFC uses `<template>` not TSX. Vite `@vitejs/plugin-vue` handles `.vue` compilation.

---

## Sample 3: Svelte SSR (`samples/vite-svelte-ssr-app/`)

**Purpose:** Svelte 5 SSR.

**Deps:** `svelte`, `@sveltejs/vite-plugin-svelte`, `vite`

**Note:** Svelte 5 [`render`](https://svelte.dev/docs/svelte-server) from `svelte/server` is synchronous — no async prerequisite needed.

**Key entry:**

```ts
import { render } from 'svelte/server'
import App from './App.svelte'

function render(argsJson: string): string {
  const { data } = JSON.parse(argsJson)
  const result = render(App, { props: { data } })
  // result: { head: string, body: string }
  return `<!DOCTYPE html>
<html>
  <head>${result.head}<title>Hello</title></head>
  <body><div id="root">${result.body}</div></body>
</html>`
}
globalThis.render = render
```

**Files to create:**

| File | Content |
|------|---------|
| [`samples/vite-svelte-ssr-app/deno.json`](../samples/vite-svelte-ssr-app/deno.json) | svelte, svelte/vite-plugin imports |
| [`samples/vite-svelte-ssr-app/vite.config.ts`](../samples/vite-svelte-ssr-app/vite.config.ts) | plugin: svelte() |
| [`samples/vite-svelte-ssr-app/tsconfig.json`](../samples/vite-svelte-ssr-app/tsconfig.json) | same |
| [`samples/vite-svelte-ssr-app/serve.deno.ts`](../samples/vite-svelte-ssr-app/serve.deno.ts) | same pattern |
| [`samples/vite-svelte-ssr-app/src/entry-server.ts`](../samples/vite-svelte-ssr-app/src/entry-server.ts) | sync render via svelte/server render() |
| [`samples/vite-svelte-ssr-app/src/App.svelte`](../samples/vite-svelte-ssr-app/src/App.svelte) | Svelte component |
| [`samples/vite-svelte-ssr-app/src/components/HelloWorld.svelte`](../samples/vite-svelte-ssr-app/src/components/HelloWorld.svelte) | child component |

---

## Sample 4: React + MUI SSR (`samples/vite-react-mui-ssr-app/`)

**Purpose:** React 19 with Material UI. Returns plain HTML — consuming app handles MUI styles.

**Deps:** `react`, `react-dom`, `@mui/material`, `@mui/icons-material`, `@emotion/react`, `@emotion/styled`, `@vitejs/plugin-react`, `vite`

**Key entry:**

```ts
import { renderToString } from 'react-dom/server'
import { createElement } from 'react'
import { CacheProvider } from '@emotion/react'
import createCache from '@emotion/cache'
import App from './App'

function render(argsJson: string): string {
  const { data } = JSON.parse(argsJson)
  const cache = createCache({ key: 'mui' })
  const html = renderToString(
    createElement(CacheProvider, { value: cache },
      createElement(App, { data })
    )
  )
  return html
}
globalThis.render = render
```

**Notes:** All samples use `ssr.resolve.conditions: ['edge-light', 'module', 'browser', 'development']`
in their Vite config to prevent packages like `@emotion/cache` from resolving to their browser build
under `ssr.target: 'webworker'`. See [`plans/archived/edge-light-resolution.md`](archived/edge-light-resolution.md).

**Files to create:**

| File | Content |
|------|---------|
| [`samples/vite-react-mui-ssr-app/deno.json`](../samples/vite-react-mui-ssr-app/deno.json) | react, react-dom, @mui, @emotion, vite imports |
| [`samples/vite-react-mui-ssr-app/vite.config.ts`](../samples/vite-react-mui-ssr-app/vite.config.ts) | plugin: react() + edge-light resolve conditions |
| [`samples/vite-react-mui-ssr-app/tsconfig.json`](../samples/vite-react-mui-ssr-app/tsconfig.json) | same |
| [`samples/vite-react-mui-ssr-app/serve.deno.ts`](../samples/vite-react-mui-ssr-app/serve.deno.ts) | standard test server |
| [`samples/vite-react-mui-ssr-app/src/entry-server.ts`](../samples/vite-react-mui-ssr-app/src/entry-server.ts) | CacheProvider + renderToString |
| [`samples/vite-react-mui-ssr-app/src/App.tsx`](../samples/vite-react-mui-ssr-app/src/App.tsx) | MUI components (Button, Typography, Card) |

## Sample 4b: React + MUI + Emotion SSR (`samples/vite-react-mui-emotion-ssr-app/`)

**Purpose:** React 19 with Material UI. Includes explicit Emotion CSS extraction.

**Deps:** Same as Sample 4 + `@emotion/cache`.

**Key entry:** Wraps with `CacheProvider`, extracts CSS from `cache.inserted` after render, returns `{html, css}` JSON.

**Files to create:**

| File | Content |
|------|---------|
| [`samples/vite-react-mui-emotion-ssr-app/deno.json`](../samples/vite-react-mui-emotion-ssr-app/deno.json) | same + @emotion/cache |
| [`samples/vite-react-mui-emotion-ssr-app/vite.config.ts`](../samples/vite-react-mui-emotion-ssr-app/vite.config.ts) | plugin: react() + edge-light resolve conditions |
| [`samples/vite-react-mui-emotion-ssr-app/tsconfig.json`](../samples/vite-react-mui-emotion-ssr-app/tsconfig.json) | same |
| [`samples/vite-react-mui-emotion-ssr-app/serve.deno.ts`](../samples/vite-react-mui-emotion-ssr-app/serve.deno.ts) | parse JSON result, render full HTML |
| [`samples/vite-react-mui-emotion-ssr-app/src/entry-server.ts`](../samples/vite-react-mui-emotion-ssr-app/src/entry-server.ts) | emotion cache + CSS extraction |
| [`samples/vite-react-mui-emotion-ssr-app/src/App.tsx`](../samples/vite-react-mui-emotion-ssr-app/src/App.tsx) | same as Sample 4 |
| [`samples/vite-react-mui-emotion-ssr-app/src/components/MuiCard.tsx`](../samples/vite-react-mui-emotion-ssr-app/src/components/MuiCard.tsx) | reusable MUI Card component |

---

## Sample 5: React + Emotion + MUI Dashboard (`samples/vite-react-emotion-mui-dashboard-ssr-app/`)

Port MUI v9.0.0 official dashboard template. Complex real-world layout with
AppBar, Drawer, DataGrid, charts, date pickers, tree view, and stat cards.

See separate plan: [`plans/vite-react-emotion-mui-dashboard-ssr-app.md`](../plans/vite-react-emotion-mui-dashboard-ssr-app.md)

---

## Rakefile Changes

Update [`Rakefile`](../Rakefile) `namespace :samples` to build all samples:

```ruby
SAMPLES = %w[
  vite-react-ssr-app
vite-ssr-app
  vite-vue-ssr-app
  vite-svelte-ssr-app
  vite-react-mui-ssr-app
  vite-react-mui-emotion-ssr-app
  vite-react-emotion-mui-dashboard-ssr-app
]

namespace :samples do
  desc 'Build all SSR sample bundles'
  task :build do
    SAMPLES.each do |sample|
      sh 'deno', 'task', 'build', chdir: "samples/#{sample}"
    end
  end

  SAMPLES.each do |sample|
    desc "Build the #{sample} SSR bundle"
    task "build:#{sample}" do
      sh 'deno', 'task', 'build', chdir: "samples/#{sample}"
    end
  end
end
```

---

## Integration Test Updates

[`test/ssr/test_integration_samples.rb`](../test/ssr/test_integration_samples.rb) — add test for each new sample:

```ruby
def test_render_vanilla_ssr
  bundle = SSR::Deno::Bundle.new('samples/vite-ssr-app/dist/server/entry-server.js')
  result = bundle.render({ name: 'World' })
  assert_includes result, 'Hello World'
end
```

---

## Implementation Order

| Step | What | Status |
|------|------|--------|
| 1 | Async render support in Rust (promise polling) | ✅ |
| 2 | Vanilla SSR sample | ✅ |
| 3 | Svelte SSR sample | ✅ |
| 4 | Vue SSR sample | ✅ |
| 5 | React + MUI SSR sample | ✅ |
| 6 | React + Emotion + MUI Dashboard sample | ✅ |
| 7 | Update Rakefile samples:build | ✅ |
| 8 | Update integration tests | ✅ |
| 9 | `bundle exec rake` — full pipeline verify | ✅ |

All samples implemented. Move plan to `plans/archived/` after confirming no open questions remain.

---

## Open Questions

1. **Vue async:** Confirmed Vue `renderToString` returns Promise → Step 1 required. Alternative: use synchronous wrapper (not possible with Vue 3 API).
2. **Svelte version:** Svelte 5's `svelte/server` render is sync. Confirm Svelte 5 is available via npm (yes).
3. **MUI X DataGrid:** May need licensing. Can use basic MUI Core Table instead if DataGrid license issue.
4. **serve.deno.ts for MUI samples:** Since MUI samples return `{html, css}` JSON, the test server needs to parse and construct full HTML. ✅ Done — both MUI samples have working `serve.deno.ts` (plain for `vite-react-mui-ssr-app`, JSON-parsing for `vite-react-mui-emotion-ssr-app`).
