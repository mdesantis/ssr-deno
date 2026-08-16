# ssr-deno project preferences

**This file is written in caveman mode.** Keep it that way — no filler, no pleasantries, fragments OK. All edits must stay terse.

## Architecture

Ruby gem embedding Deno V8 via Rust native ext (`ext/ssr_deno/`). No subprocess, no HTTP bridge. Vite SSR bundles loaded into V8 isolates.

**Boundary:** `lib/ssr/deno/` (Ruby API) ↔ `ext/ssr_deno/src/` (Rust/magnus).
**Pure-Rust types:** `ext/ssr_deno/crates/ssr_deno_core/` — no V8 dep, fast compile.
**Rust edition/MSRV:** shared via `[workspace.package]` in `ext/ssr_deno/Cargo.toml`. MSRV is `rust-version = "1.95"`, CI-verified (`msrv` job in `ci.yml`, pinned toolchain, not floating `@stable`) — not aspirational. Bumping any pinned Deno/V8 crate can silently raise the real floor higher than the declared one (happened when establishing it — a chain of transitive deps forced 1.95, ten minors above what the top-level crate's own edition alone implied). Re-run `RUSTUP_TOOLCHAIN=<candidate> bundle exec rake compile` after any dep bump before assuming `1.95` still holds — if the floor moves, update both `Cargo.toml`'s `rust-version` and the `msrv` job's `dtolnay/rust-toolchain@<version>` pin + job name in `ci.yml` together, or CI silently stops verifying the real floor.

## Conventions

- **`SSR` always fully uppercased.** Never `Ssr` — including class names like `TestIntegrationReactSSR`.
- **"stream/streaming" banned from internal code.** Internal identifiers use domain-accurate names: `render_chunks`, `__ssr_deno_result`, `__SSR_DENO_SENTINEL`, `ssr_deno_ops`. Allowed only in: user-facing docs, `node:stream` module names, sample dir names, archived plans, Rails `response.stream`.
- **Diagrams must be Mermaid.** Use ` ```mermaid ` blocks. No hand-crafted Unicode box art. File trees (`├──`) are not diagrams — plain text OK.
- **File deletion in Ruby: use `FileUtils.rm_f` (files) or `FileUtils.rm_rf` (dirs).** Never `File.delete` with existence check (`File.delete(path) if File.exist?(path)`). `rm_f`/`rm_rf` is atomic and handles all edge cases.
- **No numbered prose in docs/comments.** Never "there are 3 ways…" or "step 1, step 2" — counts go stale. Ordered lists (`1. 2. 3.`) are fine. Plans exempt.
- **Doc audit before every change.** Identify which docs/comments/RBS/plans could go stale. Update in lockstep — not after.
- **Plan step = complete only when all dependencies are ✅.** Use ◐ (partial) if deps open. Use ❌ for rejected steps. Move plan to `plans/archived/` only when fully done.
- **`plans/archived/` is history — never edit it.** Archived plans record what was decided and why at the time; a later plan superseding one does not make it wrong, just old. Record supersession in the *live* plan that replaces it, and correct stale claims in whatever live doc repeats them. (`ci-speedup.md` carries a supersession note added before this rule existed — left in place, not a precedent.)
- **Link into `plans/archived/` with absolute GitHub URLs from anything shipped.** `plans/` is not in `spec.files` (`ssr-deno.gemspec`), so a relative link from `docs/*.md`, `README.md`, or RDoc in `lib/**` is dead in the installed gem. Use `https://github.com/mdesantis/ssr-deno/blob/main/plans/archived/<file>.md`. Relative links are fine between files that ship together, and inside `plans/` itself.

## Workflow
- **`bundle exec rake` — only valid full-pipeline command.** See the `default` task in `Rakefile` for the exact chain and order — not duplicated here, it goes stale. Never `bundle exec rake test` or subset.

- **Before `bundle exec rake`, run all default steps sequentially first.** Take the list and order from the `default` task in `Rakefile` — not repeated here, for the same reason as above. Run each step independently. **Check exit status (`echo $?`)** after each (run command standalone, never piped to `tail`/`grep` — pipe masks exit code). Fix any failure before the final `bundle exec rake`. This avoids wasting time on a long pipeline that aborts on step N and also catches false failures from stale coverage data. **After all sequential steps pass, run `bundle exec rake` once as the final confirmation.**
- **Check assignment-blank-line rule before running rake.** Read every modified Ruby file. Fix violations first.
- **Never auto-commit.** Only commit when asked ("commit please"). Show `git diff --cached` and wait for confirmation.
- **Fixup before push.** If staged changes are strictly related to the previous commit and that commit wasn't pushed yet, amend instead of creating a new commit. Exception: archival always gets its own commit (rename + reference updates together).
- **Use `caveman-commit` skill for commit messages.** Conventional Commits, subject ≤50 chars, body only for non-obvious why.
- **Compile with `bundle exec rake compile`.** Never raw `cargo build` — skips linker flags, Ruby can't load result. Sole exception: the `Dockerfile`'s builder stage, which runs before the Rakefile is copied and renames the `.so` by hand (see the comment there).
- **Keep `sig/ssr/deno.rbs` in sync.** Update in same step as any method signature/type/exception change.
- **Archiving plans: stage both new file and old-path deletion.** Use `git mv` or add deletion explicitly. Update all references to old path.
- **Release workflow:**
  - Bump `version` in `[workspace.package]` (`ext/ssr_deno/Cargo.toml`) and `lib/ssr/deno/version.rb`. Member crates inherit via `version.workspace = true` — don't add per-crate `version` back.
  - Run `bundle install` → commit `Gemfile.lock`.
  - Move `## Unreleased` to `## [version] - YYYY-MM-DD`, add fresh empty `## Unreleased` on top.
  - Tag commit (e.g. `v0.1.0-alpha.4`).
- **Stale audit after every changeset.** Check before marking complete:
  - `README.md`, `plans/*.md`, `CHANGELOG.md`, source comments, `docs/*.md`, `rakelib/*.rake` header comments, `.github/workflows/*.yml` (all three carry load-bearing rationale, and `docker-publish.yml`'s Ruby matrix claims to track `ci.yml`'s), test files, sample files/dirs, `.vscode/settings.json` (`deno.enablePaths` — matched by the global excludes file, so already-tracked edits commit normally but a new file there needs `git add -f`).
  - When adding/renaming/deleting samples: `rg` across non-vendor/non-generated repo for stale path refs.
- **RuboCop: auto-correct first.** `[Correctable]` offenses → `bundle exec rubocop -a <file>` (safe) or `-A` (all). Manual edit only if auto-correct fails.
- **TDD when step is testable.** Write failing test → implement → verify pass. If expected-fail test passes immediately, investigate before implementing. Fast loop: `bundle exec rake test`; full gate: `bundle exec rake`.
- **Mark plan steps complete immediately** after `bundle exec rake` passes for that step.
- **After completing plan + committing, propose archive.** Plan committed alongside code. Archive in separate commit, only with user confirmation.

## Setup

- **`.env` required.** `cp .env.example .env`. Defaults: `RB_SYS_CARGO_PROFILE=dev`.
- **Prerequisites:** Ruby 3.3+, Rust toolchain, Bundler, Deno 2.x.
- **Setup:** `bin/setup`. Console: `bin/console`.

## Test architecture

Each suite runs as a separate Ruby process (avoids pool re-initialization —
the pool is permanent once created) writing its own generated runner to
`tmp/test_runner_*.rb`. See `rakelib/test.rake` for the suite list and the
exact per-suite config — don't duplicate it here, it goes stale.

`bundle exec rake test` runs all of them except `test:perf` (which runs
standalone via `perf:check`) and merges coverage; `coverage:check` enforces
100% line + 100% branch on the merged result.

## Code style — assignment blank line rule

Assignment lines (`=`, `||=`, `+=`, etc.) must be separated from non-assignment lines by blank lines. Consecutive assignments group without blanks.

```ruby
# ✅
a = 1
b = 2

puts a
puts b
```

```ruby
# ❌ — assignment immediately followed by non-assignment
bundle = Object.new
@registry.register(:application, bundle)
```

```ruby
# ✅
bundle = Object.new

@registry.register(:application, bundle)

assert_same bundle, @registry[:application]
```

```ruby
# ✅
orig_mtime = @bundle.instance_variable_get(:@mtime)

FileUtils.touch(BUNDLE_PATH)
@bundle.reload

new_mtime = @bundle.instance_variable_get(:@mtime)

assert_operator new_mtime, :>, orig_mtime
```

## Pre-completion gate

Re-read this file, then execute every applicable item:

1. **Assignment-blank-line rule** — read every modified Ruby file, verify compliance
2. **`bundle exec rake`** — must exit 0
3. **`sig/ssr/deno.rbs`** — in sync with any signature/type changes
4. **Stale docs/plans/comments audit** — all modified areas
5. **`CHANGELOG.md`** — entry only for user-facing changes: published gem contents, runtime behavior, public API, README/`docs/*.md` corrections, or Docker-image build/runtime changes. Not for contributor/CI-facing work — CI workflow behavior, `rakelib`/test infrastructure, `AGENTS.md`, plan-file bookkeeping, comment resyncs get no entry even when they took real effort. State the change, not how it was verified — "verified with a full `docker build`" or "confirmed via CI" describes process, not the change; the reader wants to know what's different, not how you checked.
