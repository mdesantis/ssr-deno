# Plan: add `html_safe` support to `render()` / `render_chunks()`

## Goal

Let users opt into `.html_safe` marking on render output via a config setting
and per-call option. The library never calls `.html_safe` by default — the
caller opts in.

## Files

| File | Change |
|------|--------|
| `lib/ssr/deno.rb` | Add `html_safe=` / `html_safe?` (pure Ruby, no Rust), default `false` |
| `lib/ssr/deno/bundle.rb` | Add `html_safe:` kwarg to `render()` + `render_chunks()` |
| `lib/ssr/deno/rails/railtie.rb` | Add `config.ssr_deno.html_safe = nil`, wire in `init_bundles` |
| `lib/ssr/deno/rails/generators/ssr/deno/templates/ssr_deno.rb` | Commented-out option |
| `sig/ssr/deno.rbs` | Type signatures |
| `test/ssr/test_integration_rails.rb` | Rails integration tests |
| `CHANGELOG.md` | Entry |
| `README.md` | Update Rails section — show `html_safe: true` option |
| `docs/csp-nonce.md` | Same update |
| `plans/html-safe-support.md` | This file — archive after commit |

## Implementation

### 1. Config: `SSR::Deno.html_safe` / `html_safe?`

```ruby
def html_safe=(value)
  @html_safe = !!value
end

def html_safe?
  return @html_safe if defined?(@html_safe)
  @html_safe = false
end
```

No env var — `html_safe` is an app-level policy, not per-environment.

### 2. `Bundle#render` — `html_safe:` kwarg

```ruby
def render(data = nil, raw_input: false, raw_output: false, html_safe: nil)
  reload_if_changed if @auto_reload

  json_input = raw_input ? data : JSON.generate(data)

  instrument 'render.ssr_deno', bundle_name: @bundle_id do
    result = SSR::Deno.native_render(@bundle_id, json_input)

    result = raw_output ? result : JSON.parse(result)

    html_safe = SSR::Deno.html_safe? if html_safe.nil?

    result = result.html_safe if html_safe && result.respond_to?(:html_safe)

    result
  end
end
```

### 3. `Bundle#render_chunks` — `html_safe:` kwarg

Each chunk gets `.html_safe` if it responds to it.

Block form — wrap the block:
```ruby
wrapped = ->(chunk) { block.call(chunk.respond_to?(:html_safe) ? chunk.html_safe : chunk) }
SSR::Deno.native_render_chunks(@bundle_id, json_input, &wrapped)
```

Enumerator form — map over enum:
```ruby
enum = SSR::Deno.native_render_chunks(@bundle_id, json_input)
enum.map { |chunk| chunk.respond_to?(:html_safe) ? chunk.html_safe : chunk }
```

### 4. Railtie

```ruby
config.ssr_deno.html_safe = nil

# in init_bundles:
SSR::Deno.html_safe = config.ssr_deno.html_safe unless config.ssr_deno.html_safe.nil?
```

### 5. Template

```ruby
# Mark render output as html_safe automatically.
# Rails.application.config.ssr_deno.html_safe = true
```

### 6. RBS

```
def self.html_safe=: (bool) -> bool
def self.html_safe?: () -> bool

def render: (?(Hash[untyped, untyped] | String) data,
             ?raw_input: bool, ?raw_output: bool,
             ?html_safe: boolish) -> untyped

def render_chunks: (?(Hash[untyped, untyped] | String) data,
                    ?raw_input: bool, ?html_safe: boolish) -> Enumerator[untyped, void]
                | (?(Hash[untyped, untyped] | String) data,
                    ?raw_input: bool, ?html_safe: boolish) { (untyped) -> void } -> nil
```

### 7. Tests (`test:main` + `test:node_builtins`)

- `test_html_safe_config_default_false` — `SSR::Deno.html_safe?` → false
- `test_html_safe_config_true` — `SSR::Deno.html_safe = true` → `result.html_safe?`
- `test_html_safe_config_true_per_call_false` — config true, per-call false → not html_safe
- `test_ssr_render_with_html_safe_option` — `html_safe: true` → `result.html_safe?`
- `test_ssr_render_without_html_safe_option` — default → not html_safe

### 8. Docs

- **README:** Show `<%= ssr_render({ page: 'home' }, html_safe: true) %>` instead of `.html_safe` call
- **csp-nonce.md:** Same pattern

### Pre-completion

Before archive: check `:nocov:` directives in `lib/ssr/deno/bundle.rb` — new `html_safe` branches may be untestable outside Rails. Audit `docs/csp-nonce.md` for stale references in stale audit step.

## Order

1. Config
2. `bundle.rb` render + render_chunks
3. Railtie + template
4. RBS
5. Tests
6. Docs + CHANGELOG
7. Archive plan
