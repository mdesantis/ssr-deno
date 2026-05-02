# Preact SSR Sample

Minimal Vite + Preact SSR sample. Same contract as `react-ssr-app` but with Preact instead of React.

**Status:** ◐ Implemented with workaround — JSX transform broken by Vite 8/Deno napi-rs compat issue (see below).

---

## Deps

| Package | Why |
|---------|-----|
| `preact` | 3 KB React alternative |
| `preact-render-to-string` | SSR: `renderToString()` |
| `vite` | Bundler |

No JSX transform plugin needed — sample uses `h()` directly instead of JSX.

---

## Files

| File | Source |
|------|--------|
| `.gitignore` | copy from sibling |
| `deno.json` | preact deps + vite |
| `vite.config.ts` | no plugins, ssr.target webworker |
| `tsconfig.json` | basic TS config |
| `serve.deno.ts` | same pattern as `react-ssr-app` PORT=3107 |
| `src/entry-server.ts` | `globalThis.render` using `preact-render-to-string` + `h()` |
| `src/App.tsx` | Simple Preact component using `h()` |

---

## SSR entry

```ts
import { renderToString } from 'preact-render-to-string'
import { h } from 'preact'
import { App } from './App.tsx'

function render(argsJson: string): string {
  const { data } = JSON.parse(argsJson)
  return renderToString(h(App, { data }))
}
globalThis.render = render
```

---

## Root cause: JSX transform blocked by Deno/napi-rs compat

Vite 8 uses Rolldown (1.0.0-rc.17) which uses OXC for built-in JSX transforms.
OXC is a Rust native addon (napi-rs). In Deno's npm compat layer, passing
complex nested JS objects to Rust bindings causes a type conversion failure.

**Proof:**
```js
// Fails in Deno:
utils.transformSync('test.tsx', code, { lang: 'tsx', jsx: { ... } })
// Works:
utils.transformSync('test.tsx', code, JSON.stringify({ lang: 'tsx', jsx: { ... } }))
```

**Attempted fixes (all failed):**
1. `@preact/preset-vite` v2.10.5 — plugin detects rolldown and sets `oxc` JSX config,
   but the native transform still fails to parse `<` as JSX token.
2. `@vitejs/plugin-react` v6.0.1 — same issue, OXC transform receives options
   as object not JSON string.
3. `@vitejs/plugin-react` + `babel: true` + `@rolldown/plugin-babel` — Babel mode
   can't engage because the built-in native transform runs first and fails.
4. `.jsx` extension — same parser error.
5. `oxc` config in vite.config.ts — doesn't override the native plugin behavior.
6. `--unstable-bare-node-builtins` flag — no effect.

**Workaround:** Use `h()` (Preact's `createElement`) directly. No JSX needed.
Build works. Bundle is 23 KB (vs 453 KB for React sample).

**When this can be fixed:**
- Vite 8 stable release (may fix how OXC options are passed to native addon)
- Rolldown update that accepts stringified transform options
- Deno update that improves napi-rs object passing
- Revisit when any of these ship.

---

## Integration

- ✅ Added to `rakelib/samples.rake`
- ✅ No `node:module` needed → test in main suite
- ✅ Integration test in `test_integration_samples.rb`
- ✅ `bundle exec rake` passes
- ◐ JSX syntax not working — sample uses `h()` instead
