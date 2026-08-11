# ssr-deno project preferences

**This file is written in caveman mode.** Keep it that way — no filler, no pleasantries, fragments OK. All edits must stay terse.

## Architecture

Ruby gem embedding Deno V8 via Rust native ext (`ext/ssr_deno/`). No subprocess, no HTTP bridge. Vite SSR bundles loaded into V8 isolates.

**Boundary:** `lib/ssr/deno/` (Ruby API) ↔ `ext/ssr_deno/src/` (Rust/magnus).
**Pure-Rust types:** `ext/ssr_deno/crates/ssr_deno_core/` — no V8 dep, fast compile.

## Conventions

- **`SSR` always fully uppercased.** Never `Ssr` — including class names like `TestIntegrationReactSSR`.
- **"stream/streaming" banned from internal code.** Internal identifiers use domain-accurate names: `render_chunks`, `__ssr_deno_result`, `__SSR_DENO_SENTINEL`, `ssr_deno_ops`. Allowed only in: user-facing docs, `node:stream` module names, sample dir names, archived plans, Rails `response.stream`.
- **Diagrams must be Mermaid.** Use ` ```mermaid ` blocks. No hand-crafted Unicode box art. File trees (`├──`) are not diagrams — plain text OK.
- **File deletion in Ruby: use `FileUtils.rm_f` (files) or `FileUtils.rm_rf` (dirs).** Never `File.delete` with existence check (`File.delete(path) if File.exist?(path)`). `rm_f`/`rm_rf` is atomic and handles all edge cases.
- **No numbered prose in docs/comments.** Never "there are 3 ways…" or "step 1, step 2" — counts go stale. Ordered lists (`1. 2. 3.`) are fine. Plans exempt.
- **Doc audit before every change.** Identify which docs/comments/RBS/plans could go stale. Update in lockstep — not after.
- **Plan step = complete only when all dependencies are ✅.** Use ◐ (partial) if deps open. Use ❌ for rejected steps. Move plan to `plans/archived/` only when fully done.

## Workflow
- **`bundle exec rake` — only valid full-pipeline command.** Runs: Rust compile + `cargo:test` + `cargo:coverage` + `cargo:clippy` + `cargo:fmt` + Vite build + Ruby tests + coverage:check + perf:check + RuboCop + rubocop:rails + RBS. Never `bundle exec rake test` or subset.

- **Before `bundle exec rake`, run all default steps sequentially first.** Compile, cargo:test, cargo:coverage, cargo:clippy, cargo:fmt, samples, Ruby tests, coverage:check, perf:check, RuboCop, rubocop:rails, RBS — each step independently. **Check exit status (`echo $?`)** after each (run command standalone, never piped to `tail`/`grep` — pipe masks exit code). Fix any failure before the final `bundle exec rake`. This avoids wasting time on a long pipeline that aborts on step N and also catches false failures from stale coverage data. **After all sequential steps pass, run `bundle exec rake` once as the final confirmation.**
- **Check assignment-blank-line rule before running rake.** Read every modified Ruby file. Fix violations first.
- **Never auto-commit.** Only commit when asked ("commit please"). Show `git diff --cached` and wait for confirmation.
- **Fixup before push.** If staged changes are strictly related to the previous commit and that commit wasn't pushed yet, amend instead of creating a new commit. Exception: archival always gets its own commit (rename + reference updates together).
- **Use `caveman-commit` skill for commit messages.** Conventional Commits, subject ≤50 chars, body only for non-obvious why.
- **Compile with `bundle exec rake compile`.** Never raw `cargo build` — skips linker flags, Ruby can't load result.
- **Keep `sig/ssr/deno.rbs` in sync.** Update in same step as any method signature/type/exception change.
- **Archiving plans: stage both new file and old-path deletion.** Use `git mv` or add deletion explicitly. Update all references to old path.
- **Release workflow:**
  - Bump **all** `Cargo.toml` files in the workspace — `ext/ssr_deno/Cargo.toml`, `ext/ssr_deno/crates/ssr_deno_core/Cargo.toml`, `ext/ssr_deno/crates/ssr_deno_sys/Cargo.toml`, `ext/ssr_deno/crates/ssr_deno_dev_mode/Cargo.toml` — and `lib/ssr/deno/version.rb`. All five must match. Partial bump breaks the workspace.
  - Run `bundle install` → commit `Gemfile.lock`.
  - Move `## Unreleased` to `## [version] - YYYY-MM-DD`, add fresh empty `## Unreleased` on top.
  - Tag commit (e.g. `v0.1.0-alpha.4`).
- **Stale audit after every changeset.** Check before marking complete:
  - `README.md`, `plans/*.md`, `CHANGELOG.md`, source comments, `docs/*.md`, `rakelib/*.rake` header comments, `.github/workflows/ci.yml`, test files, sample files/dirs, `.vscode/settings.json` (`deno.enablePaths` — gitignored but commit with `git add -f`).
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

9 suites, each a separate Ruby process (avoids pool re-initialization —
the pool is permanent once created) writing its own generated runner to
`tmp/test_runner_*.rb`. See `rakelib/test.rake` for the exact per-suite
config — don't duplicate the list here, it goes stale.

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
5. **`CHANGELOG.md`** — entry only for user-facing changes: published gem contents, runtime behavior, public API, README/`docs/*.md` corrections, or Docker-image build/runtime changes. Not for contributor/CI-facing work — CI workflow behavior, `rakelib`/test infrastructure, `AGENTS.md`, plan-file bookkeeping, comment resyncs get no entry even when they took real effort.
