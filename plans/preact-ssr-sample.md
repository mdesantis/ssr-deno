# Preact SSR Sample

Minimal Vite + Preact SSR sample. Same contract as `react-ssr-app` but with Preact instead of React.

**Status:** ✅ JSX + full Preact SSR working. Uses `resolve.alias` to bridge Deno import map gap.

---

## Deps

| Package | Why |
|---------|-----|
| `preact` | 3 KB React alternative |
| `preact-render-to-string` | SSR: `renderToString()` |
| `vite` | Bundler |

No plugins needed. Aliases are set in `vite.config.ts` via `resolve.alias`.

---

## Files

| File | Source |
|------|--------|
| `.gitignore` | copy from sibling |
| `deno.json` | preact deps + vite |
| `vite.config.ts` | resolve.alias for preact/compat, ssr.target webworker |
| `tsconfig.json` | basic TS config |
| `serve.deno.ts` | same pattern as `react-ssr-app` PORT=3107 |
| `src/entry-server.tsx` | `globalThis.render` using JSX + Preact SSR |
| `src/App.tsx` | Simple Preact component with JSX |

---

## SSR entry

```tsx
import { renderToString } from 'react-dom/server'
import { App } from './App.tsx'

function render(argsJson: string): string {
  const { data } = JSON.parse(argsJson)
  return renderToString(<App data={data} />)
}
globalThis.render = render
```

---

## Vite + Deno import map gap

**Problem:** Rolldown's native Rust resolver (used by Vite 8 for module resolution)
doesn't read Deno import maps (`deno.json` imports). When packages are aliased
(e.g. `react → preact/compat`), the native addon looks for files on the
filesystem at the original name path — which doesn't exist.

**Fix:** Use Vite's `resolve.alias` instead of Deno's import map:

```ts
// vite.config.ts
export default defineConfig({
  resolve: {
    alias: {
      'react-dom/server': 'preact-render-to-string',
      'react-dom': 'preact/compat',
      'react': 'preact/compat',
      'react/jsx-runtime': 'preact/jsx-runtime',
    },
  },
  // ...
})
```

This works because `resolve.alias` is applied at the Vite/JS level before the
native resolver is invoked.

Related Deno issue: [denoland/deno#33787](https://github.com/denoland/deno/issues/33787)

---

## Integration

- ✅ Added to `rakelib/samples.rake`
- ✅ No `node:module` needed → test in main suite
- ✅ Integration test in `test_integration_samples.rb`
- ✅ JSX syntax working via `resolve.alias` workaround
- ✅ `bundle exec rake` passes
