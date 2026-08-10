# Dev-mode CJS interop — upstream follow-ups

_Extracted from `plans/archived/dev-mode-cjs-interop-bug.md` on 2026-08-10 — those items were genuinely outstanding, not "archive only when fully done" per `AGENTS.md`._

Production currently depends on the `warm_cjs_cache` workaround in
`ext/ssr_deno/src/engine/dev_load.rs`, which sidesteps an upstream V8
re-entrancy bug by keeping `require()` calls outside V8's module-evaluation
post-order walk (see the full writeup and root-cause investigation in
[`plans/archived/dev-mode-cjs-interop-bug.md`](archived/dev-mode-cjs-interop-bug.md)).
The in-repo reproducer lives at `ext/ssr_deno/src/cjs_interop_repro_test.rs`
and, as of this refresh, runs as part of `cargo:test:ssr_deno` — 7 live
regression tests guard the workaround, plus 1 `#[ignore]`d test
(`bug_entry_body_skipped`) that demonstrates the underlying bug directly.

## Action items

- [ ] Build standalone Rust repro (separate Cargo project, not embedded in this gem) — porting instructions are in `cjs_interop_repro_test.rs`'s doc header
- [ ] Test against Deno CLI to determine if embedder-specific, or reproducible with plain `deno run`
- [ ] File upstream issue at `denoland/deno` with `embedder` label, link to the standalone repro — existing references ([Discussion #23468](https://github.com/denoland/deno/discussions/23468), [Issue #28919](https://github.com/denoland/deno/issues/28919)) are adjacent, not this exact bug
- [◐] Decide: revert step 13 vs leave wired with docs — in practice, "leave wired with docs" is what shipped (`warm_cjs_cache` + doc comments pointing back at the archived plan), but that choice was never formally recorded against this checklist item

## Revisit trigger

Any of: upstream fixes the re-entrancy bug (check via the standalone repro before removing `warm_cjs_cache`), the workaround causes a new production issue, or someone has time to file the upstream issue.

## Cross-references

- [`plans/archived/dev-mode-cjs-interop-bug.md`](archived/dev-mode-cjs-interop-bug.md) — full investigation, root cause, workaround design
- [`plans/archived/ssr-source-dev-mode.md`](archived/ssr-source-dev-mode.md) step 13 — the implementation that hit this wall
- [`plans/archived/dev-mode-followups.md`](archived/dev-mode-followups.md) — non-blocking dev-mode cleanups
- [`plans/dev-mode-deferred.md`](dev-mode-deferred.md) — other open dev-mode backlog items
