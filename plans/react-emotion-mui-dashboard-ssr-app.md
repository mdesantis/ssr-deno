# React Emotion MUI Dashboard SSR Sample

Port MUI v9.0.0 official dashboard template as an SSR sample.

Source: https://github.com/mui/material-ui/tree/v9.0.0/docs/data/material/getting-started/templates/dashboard

---

## Scope

Copy ~30 files from MUI's template into `samples/vite-react-emotion-mui-dashboard-ssr-app/`:
- `Dashboard.tsx` — main component
- `components/*.tsx` (20 files) — AppNavbar, SideMenu, Header, MainGrid, StatCard, charts, etc.
- `theme/customizations/*.ts` (5 files) — charts, dataGrid, datePickers, treeView
- `shared-theme/*.tsx` (from `templates/shared-theme/`) — AppTheme, ColorMode*, themePrimitives

Add SSR entry wrapper (emotion cache + renderToString + CSS extract).
Add scaffold files (deno.json, vite.config.ts, tsconfig.json, serve.deno.ts, .gitignore).

---

## Additional npm deps

Beyond `vite-react-mui-emotion-ssr-app` baseline:

| Package | Version |
|---------|---------|
| `@mui/x-charts` | ^7.0.0 |
| `@mui/x-data-grid` | ^7.0.0 |
| `@mui/x-date-pickers` | ^7.0.0 |
| `@mui/x-tree-view` | ^9.0.0 |
| `dayjs` | ^1.11.0 |

Use community edition `@mui/x-data-grid`, NOT `@mui/x-data-grid-pro` (MIT license).

---

## Adaptations from source

1. **`Dashboard.tsx`** — replace `../shared-theme/AppTheme` import with `./shared-theme/AppTheme`
2. **`theme/customizations/dataGrid.ts`** — change `@mui/x-data-grid-pro` type augmentation to `@mui/x-data-grid`
3. **`entry-server.ts`** — new file, wraps Dashboard in Emotion CacheProvider, extracts CSS
4. **Remove `Title.tsx.preview`** — not needed, it's a demo preview asset
5. **Remove `README.md`** — not needed in sample
6. **`serve.deno.ts`** — same pattern as `vite-react-mui-emotion-ssr-app` (JSON parse `{html, css}`)

---

## SSR entry pattern

```ts
// src/entry-server.ts
import { renderToString } from 'react-dom/server'
import { createElement } from 'react'
import { CacheProvider } from '@emotion/react'
import createCache from '@emotion/cache'
import createEmotionServer from '@emotion/server/create-instance'
import Dashboard from './Dashboard.tsx'

function render(argsJson: string): string {
  const { data } = JSON.parse(argsJson)
  const cache = createCache({ key: 'dash' })
  const { extractCriticalToChunks, constructStyleTagsFromChunks } = createEmotionServer(cache)
  const html = renderToString(
    createElement(CacheProvider, { value: cache },
      createElement(Dashboard, { data })
    )
  )
  const emotionChunks = extractCriticalToChunks(html)
  const css = constructStyleTagsFromChunks(emotionChunks)
  return JSON.stringify({ html, css })
}
globalThis.render = render
```

---

## Files to create

| File | Source |
|------|--------|
| `samples/vite-react-emotion-mui-dashboard-ssr-app/.gitignore` | copy from sibling sample |
| `samples/vite-react-emotion-mui-dashboard-ssr-app/deno.json` | baseline + new deps |
| `samples/vite-react-emotion-mui-dashboard-ssr-app/vite.config.ts` | same as emotion sample |
| `samples/vite-react-emotion-mui-dashboard-ssr-app/tsconfig.json` | same as emotion sample |
| `samples/vite-react-emotion-mui-dashboard-ssr-app/serve.deno.ts` | same pattern, PORT=3110 |
| `samples/vite-react-emotion-mui-dashboard-ssr-app/src/entry-server.ts` | new (SSR wrapper) |
| `samples/vite-react-emotion-mui-dashboard-ssr-app/src/Dashboard.tsx` | from MUI template |
| `samples/vite-react-emotion-mui-dashboard-ssr-app/src/components/*.tsx` (20) | from MUI template |
| `samples/vite-react-emotion-mui-dashboard-ssr-app/src/theme/customizations/*.ts` (5) | from MUI template |
| `samples/vite-react-emotion-mui-dashboard-ssr-app/src/shared-theme/*.tsx` (4) | from MUI `templates/shared-theme/` |
| `samples/vite-react-emotion-mui-dashboard-ssr-app/src/internals/components/` | from MUI template |
| `samples/vite-react-emotion-mui-dashboard-ssr-app/src/internals/data/` | from MUI template |

---

## Implementation

1. Create scaffold files (deno.json, vite.config.ts, tsconfig.json, .gitignore, serve.deno.ts)
2. Fetch and write all MUI template source files
3. Create src/entry-server.ts (SSR wrapper)
4. Build: `deno task build`
5. Update `rakelib/samples.rake` — add dashboard to SAMPLES
6. Update test/ssr/test_integration_samples.rb — add dashboard test
7. Run `bundle exec rake` (full pipeline, needs `node_builtins: true`)

---

## Verification

1. `deno task build` succeeds
2. `SSR::Deno::Bundle.new(path).render({})` returns `{html, css}` JSON
3. Integration test asserts MUI dashboard components rendered
4. `bundle exec rake` passes
