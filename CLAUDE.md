# ssr-deno project preferences

## Conventions

- **`SSR` Ruby constant is always uppercased.** `SSR` is the acronym of "Server-Side Rendering". Never use `Ssr`.

## Workflow

- **Always run `bundle exec rake` (full pipeline) after any Ruby file changeset.** Never run `bundle exec rake test` or other subset — only the full `bundle exec rake` counts. This runs: compilation (Rust native extension), Vite SSR sample build, tests, RuboCop linting, SimpleCov coverage check (must be 100% line + 100% branch), and RBS signature validation. Do not consider a changeset complete until `bundle exec rake` exits 0.
- **Before running `bundle exec rake`, verify every changed file complies with the assignment-blank-line rule** (see Code style section below). Read each modified file and check every assignment line has a blank line before the next non-assignment line. Fix violations before running the pipeline.
- **Never auto-commit.** Only commit when explicitly asked with "commit please" or similar.
- **Compile with `bundle exec rake compile`, never raw `cargo build`.** Rake wires the correct linker flags and installs the `.so` into `lib/ssr/deno/` where Ruby can load it. Plain `cargo build` skips that and produces an artifact Ruby cannot load.
- **Keep `sig/ssr/deno.rbs` in sync.** When changing method signatures, return types, or exception classes in `lib/ssr/deno.rb` or `ext/ssr_deno/src/lib.rs`, update `sig/ssr/deno.rbs` in the same step.
- **Check for stale docs, plans, and comments after every changeset.** Before marking any task complete, audit all modified areas for content that no longer matches the code. This includes:
  - `README.md` — usage examples, API references, setup instructions
  - `plans/*.md` — architecture docs, integration plans, security reviews
  - `CHANGELOG.md` — missing entries for new features, fixes, or breaking changes
  - Source file comments — stale references to old APIs, renamed types, or outdated reasoning
  - `lib/ssr/deno/bundle.rb` — `:nocov:` directives that may have drifted from their intended scope
  - `.github/workflows/ci.yml` — steps that may be missing or out of sync with `Rakefile`
  - Test files — stale run instructions, wrong file paths in comments
  - Sample files — comments referencing old crate names or APIs
  Do not consider the changeset complete until this audit passes.

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

Before calling `attempt_completion`, you **must** re-read this file from the top and execute every applicable item in the Workflow section. This is not optional. The checklist items that apply to every changeset are:

1. **Assignment-blank-line rule** — read every modified Ruby file and verify compliance
2. **`bundle exec rake`** (full pipeline) — must exit 0
3. **`sig/ssr/deno.rbs`** — verify it's in sync with any signature/type changes
4. **Stale docs/plans/comments audit** — check all modified areas per the list above
5. **`CHANGELOG.md`** — if the change is user-facing, add an entry

Do not skip any step. Do not assume a step passes without verifying.
