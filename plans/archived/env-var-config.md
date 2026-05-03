# Env var-based config for SSR::Deno settings

## Summary

Add support for environment-variable-based configuration of the 4 native `SSR::Deno` settings. Env vars act as **defaults** — explicit setter API calls override them. Also add getter methods so users can introspect current config.

## Env var names

| Env var | Setting | Type | Native default |
|---|---|---|---|
| `SSR_DENO_MAX_HEAP_SIZE_MB` | `max_heap_size_mb` | Integer (MB) | 64 |
| `SSR_DENO_ISOLATE_POOL_SIZE` | `isolate_pool_size` | Integer | 0 (auto) |
| `SSR_DENO_RENDER_TIMEOUT_MS` | `render_timeout_ms` | Integer (ms) | 500 |
| `SSR_DENO_NODE_BUILTINS_ENABLED` | `node_builtins_enabled` | Boolean | false |

## Implementation steps

### 1. Rust: Add native getter functions (`ext/ssr_deno/src/lib.rs`) ✅
### 2. Ruby: Add getter methods (`lib/ssr/deno.rb`) ✅
### 3. Ruby: Add env var defaults (`lib/ssr/deno.rb`) ✅
### 4. RBS: Add getter signatures (`sig/ssr/deno.rbs`) ✅
### 5. Tests (`test/ssr/test_deno_env_config.rb`) ✅
### 6. Test runner (`rakelib/test.rake`) ✅
### 7. Documentation ✅

- **`README.md`** — add "Environment variables" subsection under Configuration.
- **`CHANGELOG.md`** — add entry under Unreleased.
- **`docs/ARCHITECTURE.md`** — update the config row for `lib/ssr/deno.rb`.

## Precedence behavior

```
Native defaults (Config::default)
  ↓ overridden by
ENV vars (at require time, if set)
  ↓ overridden by
Setter API calls (e.g., SSR::Deno.max_heap_size_mb = 256)
  ↓ applied at
Pool initialization (first Bundle.new)
```

Rails railtie flow: `ENV` → `require "ssr/deno"` (env vars applied) → railtie `after_initialize` calls setters conditionally (only if `config.ssr_deno.max_heap_size_mb` is non-nil) → setter overrides env → pool init.

## Edge cases

| Case | Behavior |
|---|---|
| Env var not set | Native default used (untouched) |
| Env var empty string (e.g., `SSR_DENO_MAX_HEAP_SIZE_MB=`) | Treated as not set, native default used |
| Env var set to valid value | Applied via setter (triggers native validation) |
| Env var set to invalid integer format (e.g., `abc`) | `warn` + skip, native default remains |
| Env var set to out-of-range value (e.g., `SSR_DENO_RENDER_TIMEOUT_MS=99`) | `ArgumentError` from Rust layer — fails at require-time |
| Env var set, then setter called | Setter wins (last-write semantics) |
| Env var set but pool already initialized | Setter raises `JsRuntimeInitializationError` |
