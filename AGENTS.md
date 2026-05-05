# ssr-deno project preferences

## Architecture

Ruby gem embedding Deno V8 runtime via Rust native extension (`ext/ssr_deno/`).
No subprocess, no HTTP bridge. Vite SSR bundles loaded directly into V8 isolates.

**Key boundary:** `lib/ssr/deno/` (Ruby API) ↔ `ext/ssr_deno/src/` (Rust/magnus) ↔ `vendor/rusty_v8/` (git submodule).
**Pure-Rust types:** `ext/ssr_deno/crates/ssr_deno_core/` — no V8 dep, fast compile.

## Conventions

- **`SSR` is always fully uppercased.** Never use `Ssr` — even in class names like `TestIntegrationReactSSR`, not `TestIntegrationReactSsr`.

- **"Streaming" / "stream" must not leak into internal code.** These are user-facing concepts describing the SSR delivery model. Internal identifiers (Rust functions, JS globals, Ruby native methods, constants, extension names) must use domain-accurate names — e.g., `render_chunks`, `__ssr_deno_result`, `__SSR_DENO_SENTINEL`, `ssr_deno_ops`. The word "stream" is acceptable only in: user-facing documentation describing the concept, Node.js built-in module names (`node:stream`), sample directory names, archived plans (historical record), and Rails API references (`response.stream`).

- **Diagrams must be Mermaid.** All diagrams in plans, docs, and comments must use ```mermaid blocks. Never use hand-crafted ASCII art with Unicode box-drawing characters. Directory listings (file trees with `├──`, `└──`, `│`) are not diagrams — they can stay as plain text.

- **No numbered prose in docs/comments.** Never write "there are 3 ways..." or "step 1, step 2" in docs, comments, or README — those go stale the moment code changes. Same for hardcoded counts (test counts, file counts, iteration numbers) — use indefinite phrasing like "multiple suites", "several tests", or describe what the tests/suites cover instead. Proper ordered lists (1. 2. 3.) are fine. Plans are exempt — numbered steps are useful there.

- **Every change must be accompanied by a documentation audit.** Before writing any code, first identify which docs, comments, README sections, RBS signatures, or plan files could become stale as a result of the change. Update them in lockstep with the code — not after. This applies to: setting accessor comments, public API docs, source-level inline comments, README usage sections, RBS type signatures, and plan files that reference the modified area. Do not leave documentation drift for a later cleanup pass.

- **Do not mark a plan step as completed if it has open dependencies.** A step should only be marked ✅ when ALL its dependency steps are also ✅. If a step has open/pending dependencies, mark it ◐ (partial) or leave it [ ] (pending). If a plan as a whole has any open steps, it should remain in `plans/` — only move fully completed plans to `plans/archived/`.

## Workflow

- **Always run `bundle exec rake` (full pipeline) after any changeset.** Never run `bundle exec rake test` or other subset — only the full `bundle exec rake` counts. This runs: compilation (Rust native extension), Rust unit tests (`cargo test -p ssr_deno_core`), Vite SSR sample build, Ruby tests, RuboCop linting, SimpleCov coverage check (must be 100% line + 100% branch), and RBS signature validation. Do not consider a changeset complete until `bundle exec rake` exits 0.

- **Before running `bundle exec rake`, verify every changed file complies with the assignment-blank-line rule** (see Code style section below). Read each modified Ruby file and check every assignment line has a blank line before the next non-assignment line. Fix violations before running the pipeline.

- **Never auto-commit.** Only commit when explicitly asked with "commit please" or similar. Before committing, always show the staged changes (`git diff --cached`) and ask for confirmation — the user must review before the commit goes through.

- **Use `caveman-commit` skill for commit messages.** When committing is requested, invoke the `caveman-commit` skill (if available) to generate ultra-compressed Conventional Commits format messages. Subject ≤50 chars, body only for non-obvious why.

- **Compile with `bundle exec rake compile`, never raw `cargo build`.** Rake wires the correct linker flags and installs the `.so` into `lib/ssr/deno/` where Ruby can load it. Plain `cargo build` skips that and produces an artifact Ruby cannot load.

- **Keep `sig/ssr/deno.rbs` in sync.** When changing method signatures, return types, or exception classes in `lib/ssr/deno.rb` or `ext/ssr_deno/src/lib.rs`, update `sig/ssr/deno.rbs` in the same step.

- **When archiving a plan to `plans/archived/`, stage both the new file AND the deletion of the old path.** Use `git mv` or add the deletion explicitly. Git only detects the rename as a rename when both the old deletion and the new file are in the same commit. Also update any references to the old path in other plan files, docs, or the consolidated plan index.

- **Release workflow:**
  - Bump `lib/ssr/deno/version.rb`, `ext/ssr_deno/Cargo.toml`, `ext/ssr_deno/crates/ssr_deno_core/Cargo.toml` (all three match).
  - After bumping versions, run `bundle install` to update `Gemfile.lock` and commit it.
  - Move `## Unreleased` content to a new `## [version] - YYYY-MM-DD` section, then add a fresh empty `## Unreleased` section on top.
  - Tag the release commit with the version (e.g., `v0.1.0-alpha.4`).

- **Check for stale docs, plans, and comments after every changeset.** Before marking any task complete, audit all modified areas for content that no longer matches the code. This includes:
  - `README.md` — usage examples, API references, setup instructions
  - `plans/*.md` — architecture docs, integration plans, security reviews
  - `CHANGELOG.md` — missing entries for new features, fixes, or breaking changes
  - Source file comments — stale references to old APIs, renamed types, or outdated reasoning
  - `lib/ssr/deno/bundle.rb` — `:nocov:` directives that may have drifted from their intended scope
  - `.github/workflows/ci.yml` — steps that may be missing or out of sync with `Rakefile`
  - Test files — stale run instructions, wrong file paths in comments
  - Sample files — comments referencing old crate names or APIs
  - Sample directories — when adding/renaming/deleting samples, walk the non-vendor, non-generated parts of the repo with `rg` to catch every stale path reference: `README.md`, `docs/architecture.md`, `CHANGELOG.md`, `rakelib/samples.rake`, `test/ssr/test_*.rb`, `.vscode/settings.json`, `plans/*.md`, and any other file referencing a sample directory by name
  - `.vscode/settings.json` — add or remove sample paths in `deno.enablePaths`. Skip this for Node.js-only samples (no Deno). **This file is gitignored but must still be committed** — use `git add -f .vscode/settings.json` when changes are staged.
  Do not consider the changeset complete until this audit passes.

- **When fixing RuboCop offenses, try auto-correct first.** If a RuboCop offense is marked `[Correctable]`, run `bundle exec rubocop -a <file>` (safe auto-correct) or `bundle exec rubocop -A <file>` (all auto-correct) instead of manually editing. Only fix manually if auto-correct fails or is unavailable.

- **When implementing a plan step, prefer TDD when the fix is testable.** When it makes sense (the step has a testable behavior change, not just mechanical refactoring), follow:
  1. Write a failing test that reproduces the issue or asserts the new behavior
  2. Write the implementation to make the test pass
  3. Verify the test passes before proceeding
  If the expected-to-fail test does NOT fail (passes without the fix), investigate the root cause before implementing — the bug may not be real, or the test may not be exercising the right path. Skip TDD for untestable changes (cosmetic cleanup, code comments, trivial renames). Use `bundle exec rake test` as the fast-feedback loop during TDD; still run the full `bundle exec rake` before committing.

- **When implementing a plan step, mark it completed in the plan file immediately.** After each implementation step passes verification (`bundle exec rake` succeeds, tests pass, coverage meets threshold), update the plan's implementation checklist — change `[ ]` to `[x]` for that step. The plan file is the authoritative source of progress. Do not leave unmarked steps behind.

- **After completing a plan and committing the changes, propose to archive it.** The plan stays in `plans/` during the implementation commit — it is committed alongside the code changes, not moved to archive first. Once the commit lands, ask the user whether to move the plan to `plans/archived/`. If confirmed, move it and make a separate commit for the archive. Do not archive without confirmation — the user may want to keep it in `plans/` for reference during follow-up work.

## Setup prerequisites

- **`.env` file required.** Run `cp .env.example .env` before any build. Defaults include `V8_FROM_SOURCE=true`, `GN_ARGS` for TLS fix, `LIBCLANG_PATH=/usr/lib/llvm-21/lib`, `RB_SYS_CARGO_PROFILE=dev`. See `.env.example` and `plans/v8-tls-issue.md`.
- **Git submodules must be initialized.** `vendor/rusty_v8/` is a submodule. Run `git submodule update --init --recursive` after clone.
- **Prerequisites:** Ruby 3.3+, Rust toolchain, LLVM/Clang 21 (for V8 build), Bundler, Deno 2.x (for samples).
- **Full setup:** `bin/setup` (installs deps, compiles native extension).
- **Interactive console:** `bin/console`.

## Test architecture

Tests run in **two separate Ruby processes** to avoid pool re-initialization:

| Suite | File | `node_builtins` | Coverage key |
|-------|------|-----------------|--------------|
| `test:main` | `tmp/test_runner_main.rb` | `false` (default) | `test:main` |
| `test:node_builtins` | `tmp/test_runner_node.rb` | `true` | `test:node_builtins` |

Each suite sets `SIMPLECOV_COMMAND_NAME` env var for distinct `.resultset.json` keys.
`test:node_builtins` merges coverage and enforces 100% line + 100% branch.

## Code style

- **Separate assignment lines from non-assignment lines with blank lines.** An "assignment line" is any line that assigns a value (`=`, `||=`, `+=`, etc.). Consecutive assignment lines are grouped together without blank lines between them.
  - ✅ Good — assignments grouped, then blank line, then non-assignment:
    ```ruby
    a = 1
    b = 2

    puts a
    puts b
    ```
  - ❌ Bad — blank line between two assignments:
    ```ruby
    a = 1

    b = 2
    puts a

    puts b
    ```
  - ✅ Good — assignment, blank line, non-assignment:
    ```ruby
    bundle = Object.new

    @registry.register(:application, bundle)

    assert_same bundle, @registry[:application]
    ```
  - ❌ Bad — assignment immediately followed by non-assignment (no blank line):
    ```ruby
    bundle = Object.new
    @registry.register(:application, bundle)

    assert_same bundle, @registry[:application]
    ```
  - ✅ Good — assignment, blank line, non-assignment, blank line, assignment, blank line, non-assignment:
    ```ruby
    orig_mtime = @bundle.instance_variable_get(:@mtime)

    FileUtils.touch(BUNDLE_PATH)
    @bundle.reload

    new_mtime = @bundle.instance_variable_get(:@mtime)

    assert_operator new_mtime, :>, orig_mtime
    ```
  - ❌ Bad — assignment immediately followed by non-assignment:
    ```ruby
    orig_mtime = @bundle.instance_variable_get(:@mtime)
    FileUtils.touch(BUNDLE_PATH)
    @bundle.reload

    new_mtime = @bundle.instance_variable_get(:@mtime)

    assert_operator new_mtime, :>, orig_mtime
    ```

## Pre-completion gate

Before calling `attempt_completion`, you **must** re-read this file from the top and execute every applicable item. This is not optional:

1. **Assignment-blank-line rule** — read every modified Ruby file and verify compliance
2. **`bundle exec rake`** (full pipeline, includes `cargo:test`) — must exit 0
3. **`sig/ssr/deno.rbs`** — verify it's in sync with any signature/type changes
4. **Stale docs/plans/comments audit** — check all modified areas per the list in Workflow
5. **`CHANGELOG.md`** — if the change is user-facing, add an entry

Do not skip any step. Do not assume a step passes without verifying.
