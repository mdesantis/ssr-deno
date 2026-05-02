# Edge-Light Resolution — Plan

Eliminate the `document` stub workaround by making Vite resolve
`@emotion/cache` to its edge-light build (no DOM access).

---

## Root Cause

`@emotion/cache` ships three production ESM builds:

| Build | File | `document` access |
|-------|------|-------------------|
| Universal | `dist/emotion-cache.esm.js` | Guarded by `typeof document !== 'undefined'` |
| Browser | `dist/emotion-cache.browser.esm.js` | Direct `document.head` access, no guard |
| Edge-light | `dist/emotion-cache.edge-light.esm.js` | **None** — zero references to `document` |

Vite's `ssr.target: 'webworker'` triggers two changes (confirmed from Vite 8
source at `node_modules/vite/dist/node/chunks/`):

1. Sets Rolldown platform to `"browser"` (line ~34124)
2. Uses `DEFAULT_CLIENT_CONDITIONS` = `["module", "browser", "development"]`
   (instead of server conditions which filter out `"browser"`)

The `"browser"` condition in Vite's resolve list causes Rolldown to resolve
`@emotion/cache` to its **browser build** — the one that accesses
`document.head` unconditionally.

---

## Proposal

Override SSR resolve conditions to add `"edge-light"` before `"browser"`.
The `@emotion/cache` exports map lists `edge-light` before `browser`, so
when both conditions are present, `edge-light` wins.

### Vite config change

```ts
export default defineConfig({
  plugins: [react()],
  ssr: {
    target: 'webworker',
    noExternal: true,
    resolve: {
      conditions: ['edge-light', 'module', 'browser', 'development'],
    },
  },
  build: {
    ssr: true,
    outDir: 'dist/server',
    rollupOptions: {
      input: 'src/entry-server.ts',
    },
  },
})
```

---

## Verification

1. Build the sample with the new config
2. Check the bundle: `grep 'isBrowser\|document.head' dist/server/entry-server.js`
   - Before fix: `isBrowser = true` and `document.head` — **browser build**
   - After fix: `isBrowser = typeof document !== 'undefined'` or no `document.head` — **edge-light build**
3. Test the SSR server: `deno task serve` — should work without `document` stub
4. Run `bundle exec rake` — full pipeline must pass

---

## Cleanup

If the fix works:

1. Remove `document` stub from `react-mui-emotion-ssr-app/src/entry-server.ts`
2. Remove `document` stub from `react-mui-ssr-app/src/entry-server.ts`
3. Update `plans/react-mui-workarounds.md` — mark workaround #1 as resolved
4. Apply the Vite config change to both MUI samples
5. Copy `.opencode/plans/edge-light-resolution.md` to `plans/edge-light-resolution.md`

---

## Risk

- `edge-light` build is designed for edge runtimes (Cloudflare Workers, Deno).
  It skips all style injection into DOM. For our use case (SSR with manual CSS
  extraction from `cache.inserted`), this is exactly what we want.
- Other dependencies might also resolve differently with `edge-light` condition.
  Check the bundle for unexpected changes.
- If `ssr.resolve.conditions` doesn't work in Vite 8, fall back to the
  `environments.ssr.resolve.conditions` API.
