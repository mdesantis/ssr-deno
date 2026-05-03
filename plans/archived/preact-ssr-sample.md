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

Related Deno issue: [denoland/deno#33787](https://github.com/denoland/deno/issues/33787) — **closed** (not a Deno bug, Rolldown limitation).

### Deep dive: who owns the fix?

Three layers, two resolution paths:

| Layer | Role | Resolution type |
|-------|------|-----------------|
| **`deno-vite-plugin`** (`enforce: "pre"`) | `resolveId` hook for `npm:`, `jsr:`, `http:`, `https:` specifiers | JS-level, runs **before** native resolver |
| **Vite `resolve.alias`** | Bare specifier mapping at Vite config level | JS-level, runs **before** native resolver |
| **Rolldown native resolver** | Direct `node_modules/` filesystem access | Rust-level, **after** JS hooks return null |

**Resolution flow:**

| Specifier type | Example | Who handles it? | Result |
|----------------|---------|-----------------|--------|
| **Prefixed** | `npm:preact`, `jsr:@std/fs` | `deno-vite-plugin` (`resolveId`) | ✅ Resolved via Deno's own module system |
| **Bare alias** | `react` → `preact/compat` | `resolve.alias` in `vite.config.ts` | ✅ Mapped at Vite/JS level before native resolver |
| **Bare alias (via import map only)** | `react` → `preact/compat` in `deno.json` | No one — falls to native resolver | ❌ Native resolver looks for `node_modules/react/` (doesn't exist) |

**Root cause:** `deno-vite-plugin` only handles specifiers with a known prefix (`npm:`, `jsr:`, `http:`, `https:`). Import map aliases use bare specifiers (e.g., `react-dom/server`), which have no prefix. The plugin's `resolveId` returns `null`, and Rolldown's native resolver does a bare filesystem lookup — which fails because Deno doesn't create symlinks for aliased package names.

**Why Rolldown won't fix it:** Native resolver does bare filesystem lookup — standard behavior for Node.js bundlers. Adding import map support would require either:
- A `resolveId` hook that JS plugins can use to resolve **all** bare specifiers before the native code runs (no such hook yet — Rolldown only runs plugins *after* native resolution for unprefixed specifiers)
- A separate Rolldown plugin that handles Deno resolution. Two exist:

| Plugin | Source | Status |
|--------|--------|--------|
| [`DenoLoaderPlugin` (native Rust)](https://github.com/rolldown/rolldown/pull/3124) | rolldown/rolldown#3124 | **Closed** — draft PR for native `jsr:`, `npm:`, `http:` support with import map parsing. Never merged. |
| [`deno-rolldown-plugin` (Wasm)](https://github.com/denoland/deno-rolldown-plugin) | denoland/deno-rolldown-plugin | **Stalled** — 13 commits, last update 2025-07-31, early stage. Uses Deno CLI compiled to Wasm. |

The Rolldown team discussed native Deno support in [rolldown/rolldown#3172](https://github.com/rolldown/rolldown/issues/3172). Community consensus: use `deno-vite-plugin` for prefixed specifiers, and `resolve.alias` for bare alias mappings. No timeline for native import map resolution.

**Deno position:** bartlomieju explicitly rejected fixing this in Deno ([denoland/deno#33787](https://github.com/denoland/deno/issues/33787#issuecomment-2940892501)). Deno creates no filesystem symlinks for import map aliases.

**Conclusion:** Vite `resolve.alias` is the only practical solution today. It maps bare specifiers at the JS layer before the native resolver ever sees them. This is what `vite-preact-ssr-app` uses.

---

## Integration

- ✅ Added to `rakelib/samples.rake`
- ✅ No `node:module` needed → test in main suite
- ✅ Integration test in `test_integration_samples.rb`
- ✅ JSX syntax working via `resolve.alias` workaround
- ✅ `bundle exec rake` passes
