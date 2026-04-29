# ssr-deno project preferences

## Conventions

- **`SSR` Ruby constant is always uppercased.** `SSR` is the acronym of "Server-Side Rendering". Never use `Ssr`.

## Workflow

- **Always run `bundle exec rake` after a changeset.** This runs the full pipeline: compilation (Rust native extension), Vite SSR sample build, tests, RuboCop linting, SimpleCov coverage check (must be 100% line + 100% branch), and RBS signature validation. Do not consider a changeset complete until `bundle exec rake` exits 0.
- **Never auto-commit.** Only commit when explicitly asked with "commit please" or similar.
- **Compile with `bundle exec rake compile`, never raw `cargo build`.** Rake wires the correct linker flags and installs the `.so` into `lib/ssr/deno/` where Ruby can load it. Plain `cargo build` skips that and produces an artifact Ruby cannot load.
- **Keep `sig/ssr/deno.rbs` in sync.** When changing method signatures, return types, or exception classes in `lib/ssr/deno.rb` or `ext/ssr_deno/src/lib.rs`, update `sig/ssr/deno.rbs` in the same step.

## Code style

- **Blank line after declarations:** Always add a blank line between a declaration line (`def`, `class`, `module`, `attr_reader`, `attr_writer`, `private`, `rescue`, `initializer`) and the next non-declaration line (first statement in body, first config line, etc.).
  - Good:
    ```ruby
    def render(data = nil, raw_input: false, raw_output: false)
      reload_if_changed if @auto_reload

      json_input = raw_input ? data : JSON.generate(data)
      result = SSR::Deno.native_render(@bundle_id, json_input)

      raw_output ? result : JSON.parse(result)
    end
    ```
  - Good:
    ```ruby
    class Railtie < Rails::Railtie
      config.ssr_deno = ActiveSupport::OrderedOptions.new
      config.ssr_deno.bundles = { application: nil }
      ...
    ```
  - Good:
    ```ruby
    attr_writer :auto_reload

    def reload
      @mtime = File.mtime(@bundle_path)

      load
    end
    ```
  - Good:
    ```ruby
    def ssr_render(data = nil, **options)
      bundle_name = options.delete(:bundle) || :application
      bundle = find_bundle!(bundle_name)

      bundle.render(data, **options).html_safe
    rescue SSR::Deno::RenderError, SSR::Deno::JsRuntimeWorkerError => error
      raise if Rails.application.config.ssr_deno.raise_on_render_error

      Rails.logger.error "..."
      ''.html_safe
    end
    ```
  - Good:
    ```ruby
    private

    def find_bundle!(bundle_name)
      bundle = SSR::Deno::Bundle.registry[bundle_name]

      unless bundle
        raise SSR::Deno::BundleNotFoundError, "..."
      end

      bundle
    end
    ```
