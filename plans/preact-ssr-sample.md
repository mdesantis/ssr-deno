# Preact SSR Sample

Minimal Vite + Preact SSR sample. Same contract as `vite-react-ssr-app` but with Preact instead of React.

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
| `serve.deno.ts` | same pattern as `vite-react-ssr-app` PORT=3106 |
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

### Deep dive: who owns the fix?

Three layers, none at fault alone:

| Layer | Role | Why it can't fix it |
|-------|------|---------------------|
| **napi-rs** | JS↔Rust bridge library | Just passes calls through. Doesn't do module resolution. |
| **Rolldown** | Rust bundler (uses napi-rs) | Reads `node_modules/` via `fs::read`/`stat` — standard behavior. Works in Node because `node_modules/react/` exists there. Not Rolldown's job to know about Deno import maps. |
| **Deno** | JS runtime | Installs npm packages locally but does NOT create fs-level symlinks for import map aliases. E.g., `react → npm:preact/compat` installs `node_modules/preact/` but not `node_modules/react/`. Native addons can't see the alias. |

**Where a fix could land:**

1. **Deno** — Best place. When `nodeModulesDir: auto` is active and an import map entry aliases one package to another, Deno could create a symlink at the source name. E.g., `node_modules/react/` → symlink to `node_modules/preact/compat/`. This costs nothing and makes native addons work without changes.

2. **Rolldown** — Could expose a JS resolve hook via napi-rs so the JS layer (which knows about import maps) can resolve specifiers before the native code reads filesystem. Deeper API change.

3. **Vite** — Already has the workaround (`resolve.alias` at JS level before native resolver runs). This is what we use.

---

## Integration

- ✅ Added to `rakelib/samples.rake`
- ✅ No `node:module` needed → test in main suite
- ✅ Integration test in `test_integration_samples.rb`
- ✅ JSX syntax working via `resolve.alias` workaround
- ✅ `bundle exec rake` passes
