# Dev mode (experimental)

> **⚠️ Experimental — not for production.**
>
> `SSR::Deno::DevModeBundle` skips the Vite (or any) bundler and loads
> source `.tsx` / `.ts` / `.js` files directly into an embedded Deno V8
> isolate, transpiling on demand. It exists to make iteration fast in
> development — not to serve traffic.
>
> Behaviour, error messages, the CJS→ESM interop shim, and the source
> file watch strategy **may change without a deprecation cycle**.
>
> **Please report bugs and rough edges** at
> <https://github.com/mdesantis/ssr-deno/issues>. Frontend dependency
> graphs are wildly varied — every reproducer helps stabilise the
> feature.

---

## What it is

A "no-build" alternative to `SSR::Deno::Bundle` for development. Instead
of loading a pre-built `dist/server/ssr.js`, `DevModeBundle` points at
your source entry (`app/frontend/entry-server.tsx` or similar) and the
native dev worker:

1. Resolves imports through `node_modules/` (Byonm) and your
   `resolve_alias` map.
2. Transpiles `.tsx` / `.ts` / ESM `.js` on demand (Deno's transpiler).
3. Wraps CommonJS `node_modules/*.js` via a synthetic `require()` shim,
   with a warmup pass before module evaluation to sidestep an upstream
   V8 re-entrancy bug.
4. Optionally watches source mtimes and respawns the worker on change.

The same `#render` / `#render_chunks` interface as `Bundle`, so swapping
between dev and prod is a one-line config change.

## When to use it

| Use it if… | Don't use it if… |
|---|---|
| You want to edit `.tsx` and see SSR output without rebuilding. | You're serving production traffic. |
| Your dependency tree is roughly MUI / Emotion / React 19 / Vue 3 / Svelte 5 — these are covered by the test matrix. | Your bundle has heavy native bindings or exotic loader chains. |
| You can afford ~1–3s cold start per worker for large dependency graphs (MUI etc.). | You need sub-100ms cold starts. |

## Requirements

- Gem built with the default Cargo features. `--no-default-features`
  strips dev mode and `DevModeBundle.new` raises `NoMethodError` on the
  missing native methods.
- `SSR::Deno::Config.*` set **before** the first `DevModeBundle.new`
  (shared `IsolatePool` config).
- A reachable `node_modules/` under `project_root`.

## Usage

```ruby
SSR::Deno::Config.node_builtins_enabled = true   # required for MUI/Emotion
SSR::Deno::Config.source_maps_enabled  = true    # resolve stack frames to .tsx

bundle = SSR::Deno::DevModeBundle.new(
  'app/frontend/entry-server.tsx',
  name: :app,
  resolve_alias: { '@' => 'app/frontend' },
  project_root: Rails.root.to_s,
)

bundle.auto_reload = true   # respawn worker on source change

html = bundle.render({ page: 'home' })
bundle.render_chunks({ page: 'home' }) { |chunk| stream.write(chunk) }
```

The bundle registers itself in `SSR::Deno::Bundle.registry[name]`, so
the Rails helper `ssr_render(name: :app, …)` resolves it without further
wiring.

### Constructor options

| Option | Default | Description |
|---|---|---|
| `bundle_path` | required (positional) | Path to the source entry (`.tsx` / `.ts` / `.js`). |
| `name:` | `bundle_path` | Registry key for `Bundle.registry` / `find_bundle!`. |
| `resolve_alias:` | `Config.dev_resolve_alias` (default `{ '@' => 'app/frontend' }`) | Path alias map. |
| `project_root:` | `Dir.pwd` | Permission boundary + `node_modules/` root. Expanded to an absolute path. |

### Auto-reload

```ruby
bundle.auto_reload = true
```

Before each render the worker checks tracked source-file mtimes
(`native_dev_check_stale`). On any change, it respawns the Deno worker
(fresh V8 isolate) and reloads the entry. A failed transpile marks the
bundle "reload pending" so the next edit retries instead of getting
stuck on a stale empty mtime cache.

Disabled by default — there's no overhead when off.

### Configuration

| Config key | Effect |
|---|---|
| `Config.render_timeout_ms` | Per-call render timeout. Changeable at runtime; no respawn required. |
| `Config.max_heap_size_mb` | V8 heap cap per worker. Frozen at worker spawn. |
| `Config.node_builtins_enabled` | Enable `node:` scheme + Node ext init services. Needed for MUI / Emotion / anything that touches `node_modules` CJS via `require()`. |
| `Config.source_maps_enabled` | Resolve V8 stack frames back to source. On by default in non-production Rails envs. |
| `Config.dev_resolve_alias` | Default alias map applied when `resolve_alias:` is omitted. |

## Known caveats

- **CJS→ESM interop is best-effort.** The synthetic `require()` shim
  handles MUI / Emotion / React-style packages, but custom loaders,
  conditional exports, or anything that mutates `module.exports` after
  `require()` returns can misbehave. File a reproducer.
- **Transpile errors leak Deno's diagnostic format.** Source-map
  resolution may or may not kick in depending on the stage that failed.
- **Cold start cost.** First render after worker spawn loads + transpiles
  the full graph. MUI dashboards land in the 1–3s range.
- **Single entry per bundle.** Multiple entry points = multiple
  `DevModeBundle` instances.
- **No production hardening.** The worker's permission set is broad
  (`project_root` filesystem read access). Don't expose dev mode to
  untrusted callers.

## Feedback

- Crashes, panics, "module not found" surprises →
  <https://github.com/mdesantis/ssr-deno/issues>.
- CJS interop edge cases → please include `package.json`, the failing
  `require()` site, and the error.
- DX feedback (auto-reload too eager, error formatting unclear, …) is
  equally welcome.

## See also

- [Bundle (stable API)](../README.md#quick-start)
- [Ractor pool (experimental)](./ractor-pool.md)
- [Architecture](./architecture.md)
