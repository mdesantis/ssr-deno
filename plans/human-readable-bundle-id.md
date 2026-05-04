# Human-readable bundle_id

## Problem

Bundle names use Ruby `object_id` (e.g. `"47278032594620"`) — opaque in logs, instrumentation events, and error messages. Not useful for debugging when multiple bundles are loaded.

## Goal

Change bundle_id format to `<basename>#<object_id>` (e.g. `"entry-server.js#47278032594620"`), giving immediate context in diagnostics while preserving uniqueness.

## Implementation

### Step 1: Ruby — new bundle_id format

[x] `lib/ssr/deno/bundle.rb:18` — change:

```ruby
# Before
@bundle_id = object_id.to_s

# After
@bundle_id = "#{File.basename(@bundle_path)}##{object_id}"
```

### Step 2: Rust — fix unsafe JS interpolation

[x] `ext/ssr_deno/src/deno_runtime_wrapper/mod.rs:502` — the namespace script uses raw string interpolation which is unsafe if bundle_id contains `"` or `\`:

```rust
// Before (unsafe with special chars)
}})("{bundle_id}");"#

// After (escaped — matches render_stream.rs pattern)
}})({bundle_id:?});"#
```

### Step 3: Rust — update stale comment

[x] `ext/ssr_deno/src/deno_runtime_wrapper/mod.rs:491` — update:

```rust
// Before
// bundle_id is validated to [a-zA-Z0-9_-] before reaching here.

// After
// bundle_id may contain dots and '#' (format: "basename#object_id").
// The :? formatting below ensures proper escaping in the JS string literal.
```

### Step 4: Tests audit

[x] Check all test files for hardcoded bundle_id format assertions. Update any that match on pure-digit IDs.

### Step 5: Docs and CHANGELOG

[x] Add CHANGELOG entry noting improved bundle naming in instrumentation/error output.

## Safety analysis

- `File.basename` strips directory separators — no `/` or `\` in the name portion
- Remaining chars from typical bundles: `[a-zA-Z0-9._-#]` — all safe in JS string literals
- Rust `:?` formatting adds proper escaping as defense-in-depth
- V8 property lookup via `v8::String::new` + `obj.get` accepts any string key
- `#` separator: not valid in filenames on most OS, visually distinct

## No-op paths

- RBS: `@bundle_id` stays `String` — no signature change
- Rust function signatures: all take `&str` / `String` — no change
- `call_render.rs` already uses `v8::String::new(scope, key)` — safe with any string
- `render_stream.rs` already uses `{:?}` formatting — already safe
