# React MUI SSR — Workarounds

> **All workarounds have been resolved.**
> - #1 (document stub): fixed via Vite edge-light resolve conditions.
> - #2 (manual CSS extraction): fixed via custom module loader + `setup_require` in Rust extension.
> - #3 (`@emotion/css` forced dep): no longer needed — works via the module loader.
> - #4 (`@emotion/cache` explicit import): still needed as a Deno import map requirement.
> See [`plans/edge-light-resolution.md`](edge-light-resolution.md) and
> [`plans/node-builtins-support.md`](node-builtins-support.md) for details.
