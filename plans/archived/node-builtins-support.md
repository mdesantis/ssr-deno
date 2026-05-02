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

Swap `vite-react-mui-emotion-ssr-app/src/entry-server.ts` back to using
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

### Phase 1 result ✅

- `serve.deno.ts` works with `createRequire` from `node:module`
- Vite config (`ssr.resolve.builtins`) preserves `require("stream")` calls
- `@emotion/server` extracts CSS correctly via `serve.deno.ts`
- Native extension test (`bundle exec rake`) passes with manual extraction

### Phase 2 blocked — `NoopModuleLoader`

The Rust extension's `build_worker` uses:
```rust
module_loader: std::rc::Rc::new(deno_runtime::deno_core::NoopModuleLoader),
```

`NoopModuleLoader` rejects ALL ES module imports (including
`import('node:module')`), causing an abort when `setup_require`
tries to load the module.

Attempted approach: evaluate an async script that calls
`await import('node:module')` and poll microtasks. This fails because
the module loader doesn't handle `node:` scheme URLs.

### Phase 2 alternatives

**A — Custom module loader**  
Create a module loader that allows `node:` scheme (built-in modules)
while rejecting everything else. This requires implementing Deno's
`ModuleLoader` trait.

**B — Provide `require` from Rust side**  
Use V8's `Function::New` to create a `require` function in Rust.
For each requested module:
- If it's a Node.js builtin, use Deno's internal APIs to return the
  module from Rust
- If it's a file/npm module, reject it (security)

This avoids the module loader entirely.

**C — Extend `NoopModuleLoader`**  
Subclass or wrap `NoopModuleLoader` to allow `node:` and `node:`
prefixed URLs while rejecting all others.

**D — Accept manual CSS extraction**  
Since `@emotion/server` is the only package needing this, and manual
CSS extraction from `cache.inserted` works correctly, keep the current
approach. `serve.deno.ts` already has `createRequire` support for
manual testing.
