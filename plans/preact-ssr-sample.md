# Preact SSR Sample

Minimal Vite + Preact SSR sample. Same contract as `react-ssr-app` but with Preact instead of React.

**Status:** ◐ Implemented with workaround — JSX transform broken (see below).

---

## Deps

| Package | Why |
|---------|-----|
| `preact` | 3 KB React alternative |
| `preact-render-to-string` | SSR: `renderToString()` |
| `vite` | Bundler |

No `@preact/preset-vite` needed — current sample uses `h()` directly instead of JSX.

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

## SSR entry (current)

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

Uses `h()` instead of JSX because the JSX transform is broken.

---

## JSX transform issue

**Blocking:** Vite 8 uses rolldown (not esbuild) for transforms. `@preact/preset-vite` doesn't handle rolldown's JSX pipeline correctly in Vite 8.

Error:
```
[builtin:vite-transform] Error: Expected `>` but found `Identifier`
  at src/entry-server.ts:6:30
    renderToString(<App data={data} />)
```

**Attempted fixes (all failed):**
1. `@preact/preset-vite` + `jsx: "react-jsx"` / `jsxImportSource: "preact"` — fails to parse JSX
2. Removing preset, keeping deno.json `jsx: "react-jsx"` / `jsxImportSource: "preact"` — same error
3. Using `.tsx` extension with various jsx config combos — rolldown rejects the JSX tokens

**Workaround:** Use `h()` (Preact's `createElement`) directly. No JSX needed. Build works. Bundle is 23 KB.

**Proper fix needed:** Either:
- Wait for `@preact/preset-vite` to support Vite 8 / rolldown
- Or configure Vite 8 to use esbuild instead of rolldown for JSX transforms (if possible)
- Or use `preact/compat` with the React JSX runtime (might work with different plugin config)

**Status:** ◐ Implementation works via `h()` workaround. JSX sample blocked until rolldown JSX compat is resolved.

---

## Integration

- ✅ Added to `rakelib/samples.rake`
- ✅ No `node:module` needed → test in main suite
- ✅ Integration test in `test_integration_samples.rb`
- ✅ `bundle exec rake` passes
- ◐ JSX syntax not working — sample uses `h()` instead
