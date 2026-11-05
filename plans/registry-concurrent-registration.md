# Bundle registry — concurrent registration during `create_bundles!`

**Status:** open · found 2026-11-05 reviewing the bundles-built-after-boot change

## Problem

`Bundle.create_bundles!` walked `@registry.each_key` under `@_create_mutex` (now a snapshot — see first fix step). `DevModeBundle#initialize` writes
`Bundle.registry[@name] = self` without that mutex. If a `DevModeBundle` is constructed on another thread while the
walk runs and its name is new, the constructing thread raises `RuntimeError: can't add a new key into hash during
iteration`.

Not a regression: the previous `transform_values!` walk had the same window. Low odds in practice — `DevModeBundle`
is usually built once, from an initializer, before the first render — but nothing enforces that.

Same unsynchronized writer also means registry access is only safe under MRI's GVL. `Bundle.fetch` reads outside the
mutex on purpose (render hot path); see its comment.

## Fix

- ✅ Iterate a snapshot in `create_bundles!`: `@registry.keys.each { |name| materialize(name) }`. Removes the raise
  outright; keys added mid-walk are picked up by `fetch` later (a `DevModeBundle` is already an instance, so
  `materialize` would skip it anyway). Shipped with the bundles-built-after-boot change, with regression test
  `test_create_bundles_tolerates_registration_mid_walk` (parks the walk via a blocking `#to_path`).
- ⬜ Add `Bundle.register(name, entry)` that writes under `@_create_mutex`. Route `DevModeBundle#initialize` and the
  Railtie's `register_bundles` through it, so every registry writer is serialized. Keep `Bundle.registry` readable
  (tests, denpro workaround) — document direct writes as unsupported.
- ⬜ Regression test for `register` racing the walk — reuse the blocking-`#to_path` trick from the snapshot test.
- ⬜ RBS (`register`), `docs/architecture.md` registry description, `docs/dev-mode.md` ("registers itself in
  `SSR::Deno::Bundle.registry`"), CHANGELOG entry (runtime behavior).

## Open question

Should `fetch` move its read under the mutex too, for non-GVL Rubies (JRuby/TruffleRuby)? Costs a lock per render.
Gem targets MRI today (native ext via magnus) — probably not worth it until another runtime is supported.
