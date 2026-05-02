# Node.js Built-in Module Support

Provide a `require` function to the bundle evaluation context so that
packages depending on Node.js built-in modules (e.g. `@emotion/server`
via `through2` → `stream`) work in our Deno SSR environment.

---

## Approach

Add a `require` function using Deno's built-in `createRequire` from
`node:module`. This lets any bundled CJS code call `require("stream")`,
`require("buffer")`, etc. at runtime without needing manual polyfill
mappings — whatever Deno supports natively becomes available.

---

## Phase 1 — `serve.deno.ts` (test server)

### Step 1: Update Vite config

Set `ssr.resolve.builtins` to preserve Node.js builtin references.

Currently Vite sets `builtins: []` when `ssr.target: 'webworker'` +
`noExternal: true` (see Vite 8 source line ~34125). This leaves broken
`require("stream")` calls in the bundle because Rolndown can't bundle
them. Explicit builtins tell Vite: "these are external — leave the
`require` as-is".

```ts
export default defineConfig({
  plugins: [react()],
  ssr: {
    target: 'webworker',
    noExternal: true,
    resolve: {
      conditions: ['edge-light', 'module', 'browser', 'development'],
      builtins: ['stream', 'buffer', 'events', 'string_decoder'],
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

### Step 2: Update `serve.deno.ts`

Create a `require` function via Deno's `createRequire` and inject it
into the `new Function()` scope:

```ts
import { createRequire } from 'node:module'

const BUNDLE_PATH = new URL("./dist/server/entry-server.js", import.meta.url);
const bundleCode = await Deno.readTextFile(BUNDLE_PATH);
const scriptCode = bundleCode.replace(/export\s+\{[^}]+\};?\s*$/, "");

const require = createRequire(BUNDLE_PATH);

const fn = new Function('require', `
  ${scriptCode}
  return typeof render !== "undefined" ? render : null;
`);
const renderFn = fn(require);
```

### Step 3: Re-enable `@emotion/server` in MUI emotion sample

Swap `react-mui-emotion-ssr-app/src/entry-server.ts` back to using
`@emotion/server/create-instance` instead of manual CSS extraction.

### Step 4: Verify

1. `deno task build` — bundle includes `require("stream")` calls
2. `deno task serve` — bundle loads without `require is not defined` error
3. `curl http://localhost:3105?name=Test` — returns HTML with Emotion styles
4. `bundle exec rake` — full pipeline passes

### Phase 1 result ✅

- `serve.deno.ts` works with `createRequire` polyfill
- `@emotion/server` loads and extracts CSS correctly via `extractCriticalToChunks`
- Native extension (`bundle exec rake`) still fails — blocked by Phase 2
- `entry-server.ts` reverted to manual extraction for pipeline compatibility

---

## Phase 2 — Rust native extension

The embedded `MainWorker` also needs `createRequire` available.

### Investigation needed

1. Is `node:module` loaded in the sandboxed
   `Permissions::none_without_prompt()` worker?
2. Can we `import { createRequire } from 'node:module'` inside a script
   evaluated via `MainWorker::execute_script`?
3. If not, can we pre-load `createRequire` in Rust code and inject it
   into the V8 global scope before evaluating the bundle?

### Approach candidates

**A — Inject via bundle code (if `node:module` is available)**
Add the `require` setup directly in `entry-server.ts`:
```ts
import { createRequire } from 'node:module'
const require = createRequire(import.meta.url)
globalThis.require = require
```
This runs before any CJS code tries to call `require()`.

**B — Inject via Rust (if `node:module` unavailable)**
In the Rust extension, load the needed Node.js modules via Deno's
module system and register them as V8 global values before evaluating
the bundle.

### Risk: Security

`createRequire` creates a `require` that can read arbitrary files.
In the Rust extension's `Permissions::none_without_prompt()` sandbox,
this might violate the no-permissions policy.

Mitigation: wrap `createRequire` to only allow accessing Node.js
built-in modules, not filesystem paths:

```ts
const require = new Proxy(origRequire, {
  apply(target, thisArg, args) {
    const id = args[0]
    const nodeBuiltins = ['stream', 'buffer', 'events', ...]
    if (nodeBuiltins.includes(id) || id.startsWith('node:')) {
      return Reflect.apply(target, thisArg, args)
    }
    throw new Error(`require("${id}") denied — not a Node.js built-in`)
  }
})
```
