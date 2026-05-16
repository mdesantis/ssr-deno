# Ractor pool (experimental)

> **⚠️ Experimental — not for production.**
>
> `SSR::Deno::RactorPool` relies on Ruby's Ractor API, which is itself
> marked experimental upstream (Ruby 3.x/4.0). The interface, error
> semantics, and instrumentation hooks may change between minor releases
> of this gem without a deprecation cycle.
>
> Use this feature for benchmarking, prototyping, or background jobs
> where you can tolerate breakage and crashes. **Please report bugs,
> performance regressions, and rough edges** at
> <https://github.com/mdesantis/ssr-deno/issues> — that's what unblocks
> a path to stable.

---

## What it is

A parallel SSR pool built on Ruby Ractors. Each Ractor pins to one V8
isolate from the gem's underlying `IsolatePool` and dispatches `render`
/ `render_chunks` calls directly to the native FFI.

Unlike `SSR::Deno::Bundle`, the Ractor pool **bypasses**:

- `ActiveSupport::Notifications` (Ractor-unsafe)
- `SSR::Deno::Bundle` (not shareable across Ractors)
- The bundle registry / Rails helper integration

It calls `SSR::Deno.native_render` / `native_render_chunks` directly.

## When to use it

| Use it if… | Don't use it if… |
|---|---|
| You need true parallel SSR (multiple renders in flight at once) on a Ruby that benefits from Ractors. | You need `ActionController::Live`-style streaming wired to instrumentation. |
| Your bundle is pure JS — no native modules with thread-local state. | You rely on `render.ssr_deno` notifications or the Rails helper. |
| You can tolerate the experimental Ractor warning on every boot. | You need bullet-proof production stability today. |

Threaded workloads also benefit from `Bundle` alone: `native_render`
releases the GVL during its blocking channel receive, so multiple Ruby
threads can dispatch concurrently to different isolates without the
Ractor machinery. Try `Bundle` first; reach for `RactorPool` when you've
measured GVL contention.

## Requirements

- Ruby 3.3+ (the class supports both the 3.x take-based Ractor API and
  the 4.0 value-based API).
- `SSR::Deno::Config.isolate_pool_size` ≥ pool size you want.
- Configure **before** the first `RactorPool.new` — pool init is lazy
  and frozen after first construction.

## Usage

```ruby
SSR::Deno::Config.isolate_pool_size = 4
SSR::Deno::Config.node_builtins_enabled = true

pool = SSR::Deno::RactorPool.new(
  bundle_path: 'dist/server/ssr.js',
  size: 4,
  auto_reload: false,
)

html = pool.render({ name: 'World' })

pool.render_chunks({ page: 'home' }) { |chunk| response.stream.write(chunk) }

pool.reload    # reload the bundle on every worker
pool.shutdown  # graceful teardown
```

### Constructor options

| Option | Default | Description |
|---|---|---|
| `bundle_path:` | required | Path to the SSR JS bundle. |
| `size:` | `1` | Number of Ractor workers. Cannot exceed `Config.isolate_pool_size`. |
| `auto_reload:` | `false` | Re-evaluate the bundle file on every render (dev only). |

### Methods

- `render(data = nil, raw_input: false, raw_output: false) → String / Object`
- `render_chunks(data = nil, raw_input: false) { |chunk| … } → nil`
  - Without a block, returns an `Array` of chunks (no `Enumerator` — Ractor channels are pull-based, the full chunk list is materialised).
- `reload` — reload bundle file across all workers.
- `shutdown` — send `:shutdown` to every worker (best-effort).
- `size` — current worker count.

## Limitations

- **No `ActiveSupport::Notifications`.** `render.ssr_deno` /
  `bundle_load.ssr_deno` are not emitted. Add your own instrumentation
  in the calling code if you need it.
- **No bundle registry.** `SSR::Deno::Bundle.registry` is not populated
  by `RactorPool`, so the Rails helper `ssr_render` cannot route to it.
- **No `Enumerator` for chunked renders.** Chunks are buffered into an
  `Array` before being yielded.
- **Single bundle per pool.** Each pool is bound to one bundle path at
  construction time. Run multiple pools for multiple bundles.
- **No co-existence with `Bundle`** on the same isolate pool. Pick one.

## Known caveats

- Ractor isolation triggers Ruby's experimental warning. Silence with
  `Warning[:experimental] = false` if it's noisy in your test output.
- A worker crash currently leaves the pool with a dead Ractor. There's
  no automatic respawn — you must `shutdown` and rebuild the pool.
- Error messages crossing the Ractor boundary lose backtraces past the
  worker's `loop_body`. Source-map resolution still works inside the
  worker, but the Ruby-side stack is shallow.

## Feedback

If you try it and hit something:

- Bug or crash → <https://github.com/mdesantis/ssr-deno/issues>
- Perf comparison vs. `Bundle` → please share numbers in an issue;
  we're collecting data to inform the path to stable.

## See also

- [Bundle (stable API)](../README.md#quick-start)
- [Dev mode (experimental)](./dev-mode.md)
- [Architecture](./architecture.md)
