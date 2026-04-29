# ssr-deno project preferences

## Conventions

- **`SSR` Ruby constant is always uppercased.** `SSR` is the acronym of "Server-Side Rendering". Never use `Ssr`.

## Workflow

- **Always run `bundle exec rake` after a changeset.** This runs the full pipeline: compilation (Rust native extension), Vite SSR sample build, tests, RuboCop linting, SimpleCov coverage check (must be 100% line + 100% branch), and RBS signature validation. Do not consider a changeset complete until `bundle exec rake` exits 0.
- **Never auto-commit.** Only commit when explicitly asked with "commit please" or similar.
- **Compile with `bundle exec rake compile`, never raw `cargo build`.** Rake wires the correct linker flags and installs the `.so` into `lib/ssr/deno/` where Ruby can load it. Plain `cargo build` skips that and produces an artifact Ruby cannot load.
- **Keep `sig/ssr/deno.rbs` in sync.** When changing method signatures, return types, or exception classes in `lib/ssr/deno.rb` or `ext/ssr_deno/src/lib.rs`, update `sig/ssr/deno.rbs` in the same step.

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
