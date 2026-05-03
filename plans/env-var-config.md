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

### 1. Rust: Add native getter functions (`ext/ssr_deno/src/lib.rs`)

Add 4 functions that read from the global `CONFIG` mutex:

- `native_get_max_heap_size_mb() -> usize`
- `native_get_isolate_pool_size() -> usize`
- `native_get_render_timeout_ms() -> u64`
- `native_get_node_builtins_enabled() -> bool`

Register them via `define_singleton_method` in the `init` function.

### 2. Ruby: Add getter methods (`lib/ssr/deno.rb`)

Add 4 getter methods inside the `class << self` block, each delegating to its native counterpart.

### 3. Ruby: Add env var defaults (`lib/ssr/deno.rb`)

After the `class << self` block, add a private method `apply_env_var_defaults` called at require-time:

- Iterate over the 4 env var names.
- For each, check `ENV[name]` — if present and non-empty, parse and call the setter.
- **Integer parsing:** `Integer(value)` with rescue for `ArgumentError` (warn and skip).
- **Boolean parsing:** `"true"`, `"1"`, `"yes"` (case-insensitive) → `true`; anything else → `false`.
- **No-op if env var not set** — native defaults remain.
- Invalid values (e.g., `SSR_DENO_RENDER_TIMEOUT_MS=99`) raise `ArgumentError` from the Rust layer.
  Bad integer format (e.g., `SSR_DENO_MAX_HEAP_SIZE_MB=abc`) prints a warning and skips.

### 4. RBS: Add getter signatures (`sig/ssr/deno.rbs`)

Add signatures for the 4 getters (boolean getter uses `?` convention).

### 5. Tests (`test/ssr/test_deno_env_config.rb`)

Subprocess-based tests (matching `test_deno_setters.rb` pattern):
- Env var sets default value
- Setter overrides env var
- Boolean parsing edge cases (`"true"`, `"1"`, `"yes"`, `"false"`, `"0"`, `"no"`, empty)
- Invalid integer format produces warning, native default stays
- Env var not set → native default used
- Env var set → pool init uses it

### 6. Test runner (`rakelib/test.rake`)

Add `test:env_config` task. Tests set env vars via Open3's env argument. Include in the `test` task dependencies.

### 7. Documentation

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
| Env var set to valid value | Applied via setter (triggers native validation) |
| Env var set to invalid integer format (e.g., `abc`) | `warn` + skip, native default remains |
| Env var set to out-of-range value (e.g., `SSR_DENO_RENDER_TIMEOUT_MS=50`) | `ArgumentError` from Rust layer |
| Env var set, then setter called | Setter wins (last-write semantics) |
| Env var set but pool already initialized | Setter raises `JsRuntimeInitializationError` |
