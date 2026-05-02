# React MUI SSR — Workarounds

Hacks that need proper resolution before this sample is production-grade.

---

## 1. Global `document` stub for Emotion `createCache`

`@emotion/cache`'s `createCache({ key })` references `document.head` at call time.
In Deno's V8 SSR context (`new Function()` evaluation), there is no DOM — `document` is
undefined.

**Workaround:** Inline a minimal `document` mock in `entry-server.ts`:

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

**Fix needed:** Provide a proper no-DOM emotion cache, or upstream a
`createCache` variant that accepts an explicit container or skips DOM
access when `document` is absent.

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
