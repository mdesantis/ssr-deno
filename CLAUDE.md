# ssr-deno project preferences

## Conventions

- **`SSR` Ruby constant is always uppercased.** `SSR` is the acronym of "Server-Side Rendering". Never use `Ssr`.

## Workflow

- **Always run `bundle exec rake` after a changeset.** This runs the full pipeline: compilation (Rust native extension), Vite SSR sample build, tests, RuboCop linting, SimpleCov coverage check (must be 100% line + 100% branch), and RBS signature validation. Do not consider a changeset complete until `bundle exec rake` exits 0.
- **Never auto-commit.** Only commit when explicitly asked with "commit please" or similar.
- **Compile with `bundle exec rake compile`, never raw `cargo build`.** Rake wires the correct linker flags and installs the `.so` into `lib/ssr/deno/` where Ruby can load it. Plain `cargo build` skips that and produces an artifact Ruby cannot load.
- **Keep `sig/ssr/deno.rbs` in sync.** When changing method signatures, return types, or exception classes in `lib/ssr/deno.rb` or `ext/ssr_deno/src/lib.rs`, update `sig/ssr/deno.rbs` in the same step.

## Code style

- **Group consecutive variable/constant assignments together (no blank lines between them).** Use blank lines to separate assignments from non-assignment code.
  - Good:
    ```ruby
    a = 1
    b = 2

    puts a
    puts b
    ```
  - Bad:
    ```ruby
    a = 1

    b = 2
    puts a

    puts b
    ```
