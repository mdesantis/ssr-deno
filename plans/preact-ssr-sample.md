# Preact SSR Sample

Minimal Vite + Preact SSR sample. Same contract as `react-ssr-app` but with Preact instead of React.

---

## Deps

| Package | Why |
|---------|-----|
| `preact` | 3 KB React alternative |
| `preact-render-to-string` | SSR: `renderToString()` |
| `@preact/preset-vite` | Vite plugin (JSX transform) |
| `vite` | Bundler |

---

## Files

| File | Source |
|------|--------|
| `.gitignore` | copy from sibling |
| `deno.json` | preact deps + vite |
| `vite.config.ts` | plugin: preact(), ssr.target webworker |
| `tsconfig.json` | jsxImportSource: preact |
| `serve.deno.ts` | same pattern as `react-ssr-app` PORT=3107 |
| `src/entry-server.ts` | `globalThis.render` using `preact-render-to-string` |
| `src/App.tsx` | Simple Preact component with props |

---

## SSR entry

```tsx
import { renderToString } from 'preact-render-to-string'
import { App } from './App.tsx'

function render(argsJson: string): string {
  const { data } = JSON.parse(argsJson)
  return renderToString(<App data={data} />)
}
globalThis.render = render
```

---

## Integration

- Add to `rakelib/samples.rake`
- No `node:module` needed → test in main suite
- Integration test in `test_integration_samples.rb`
- `bundle exec rake` verify
