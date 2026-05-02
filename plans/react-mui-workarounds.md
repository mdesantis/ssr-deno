# React MUI SSR — Workarounds

> **Workaround #1 (document stub) has been resolved.**
> See [`plans/edge-light-resolution.md`](edge-light-resolution.md) for details.

---

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
