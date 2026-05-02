# React MUI SSR — Workarounds

Hacks that need proper resolution before this sample is production-grade.

---

## 1. Global `document` stub for Emotion `createCache`

`@emotion/cache`'s `createCache({ key })` accesses `document.head` at call time.
In Deno's V8 SSR context (`new Function()` evaluation), there is no DOM — `document`
is undefined.

### Root cause

`@emotion/cache` ships three builds:

| Build | File | Has `isBrowser` guard? |
|-------|------|------------------------|
| Universal | `dist/emotion-cache.esm.js` | ✅ `var isBrowser = typeof document !== 'undefined'` |
| Browser | `dist/emotion-cache.browser.esm.js` | ❌ hardcodes browser behavior, no guard |
| Edge-light | `dist/emotion-cache.edge-light.esm.js` | ✅ similar guard |

The package's `exports` map resolves differently depending on Vite's resolve
conditions. Our Vite config sets `ssr.target: 'webworker'` — this causes Vite
to resolve `@emotion/cache` to the **browser build** (`emotion-cache.browser.esm.js`),
which has **no `isBrowser` guard** and assumes `document` exists unconditionally.

Proof: the bundled output contains `var isBrowser = true;` — the browser build
is hardcoded to `true` because `@emotion/cache`'s browser variant skips the
runtime check entirely.

**Why the `rails_demo` project doesn't need this stub:**
rails_demo runs SSR via a Node.js Express server using Vite's dev server
(`vite.ssrLoadModule`). It does NOT set `ssr.target: 'webworker'`. Vite resolves
`@emotion/cache` to the **universal build** (`emotion-cache.esm.js`), which has
`var isBrowser = typeof document !== 'undefined'`. In Node.js, `document` is
undefined, so `isBrowser` is `false` and the DOM-accessing code is skipped.

### Workaround

Inline a minimal `document` mock in `entry-server.ts`:

```ts
const doc = globalThis as Record<string, unknown>
if (typeof doc.document === 'undefined') {
  const head = { appendChild: () => {} }
  const el = () => ({
    appendChild: () => {}, setAttribute: () => {},
    style: {}, addEventListener: () => {}, removeEventListener: () => {},
  })
  doc.document = {
    head, createElement: el,
    querySelectorAll: () => [], querySelector: () => null,
    createTextNode: () => ({}),
  }
}
```

### Fix options

1. **Change `ssr.target`** — drop `'webworker'` so Vite resolves universal
   builds. This may affect other dependencies that expect a web worker target.
2. **Custom resolve conditions** — configure Vite to prefer the universal
   build even with `webworker` target.
3. **Remove the need** — determine if MUI/Emotion CSS extraction can be done
   without calling `createCache` during SSR (like rails_demo's approach:
   `createEmotionCache` is only called client-side, SSR returns plain HTML).

---

## 2. Avoided `@emotion/server` (Node.js streams dependency)

`@emotion/server` depends on `through2` → `multipipe` → `html-tokenize` → Node.js
built-in modules (`stream`, `buffer`, `events`). Vite externalizes these, leaving
`require("stream")` calls in the bundle — which fail at runtime because the
`new Function()` evaluation context has no `require`.

**Workaround:** Replaced `extractCriticalToChunks` + `constructStyleTagsFromChunks`
with manual CSS extraction from `cache.inserted`:

```ts
function extractEmotionStyles(cache) {
  const inserted = cache.inserted
  const styles = []
  for (const id of Object.keys(inserted)) {
    if (typeof inserted[id] === 'string') styles.push(inserted[id])
  }
  return styles.join('')
}
```

**Fix needed:** Either:
- Write a pure-JS `@emotion/server` replacement that doesn't use Node streams
- Or determine if this manual extraction is sufficient for all MUI CSS

---

## 3. `@emotion/css` as forced dependency

`@emotion/server` lists `@emotion/css` as an optional peer dependency, but the
emotion-server browser ESM entry (`dist/emotion-server.browser.esm.js`) does
`import { cache } from '@emotion/css'` unconditionally. Without it in
`deno.json` imports, the Vite/Rolldown bundler errors with
`"cache" is not exported by "__vite-optional-peer-dep:@emotion/css:@emotion/server"`.

**Workaround:** Added `@emotion/css` to `deno.json` imports despite not using it
directly.

**Fix needed:** This goes away if we fully replace `@emotion/server` (see #2).

---

## 4. `@emotion/cache` explicit import

`@emotion/cache` is an indirect dependency (pulled by `@emotion/react`), but
Vite cannot resolve it from user source code (`createEmotionCache.ts`) unless
it's listed in `deno.json` imports.

**Workaround:** Added explicit `"@emotion/cache": "npm:@emotion/cache@^11.14.0"`
to `deno.json`.

**Fix needed:** Investigate whether Deno's import map resolution should
auto-expose transitive dependencies, or if this is a Vite+Rolldown limitation.
