# CSP Nonce Support

Add Content-Security-Policy nonce support for inline `<script>` tags in SSR output.

Extracted from `rails-integration.md` Phase 3 — see that plan for full context.

---

## Problem

Some SSR frameworks (e.g. Emotion, styled-components) inject inline `<script>` tags into HTML output. When CSP is enabled, these inline scripts are blocked unless they carry a valid `nonce` attribute matching the response's CSP header.

## Requirements

- `ssr_render` helper must accept a `nonce:` option
- Nonce value passed through to rendered HTML `<script>` tags
- Compatible with Rails CSRF protection's `content_security_policy_nonce` helper
- CSP nonce from controller flows through to SSR output
- Default: no nonce (backward-compatible, no CSP enforcement)

## Approach

### Option A: Pass nonce through render data

Add `nonce` to the data hash sent to JS `render` function. JS-side entry point reads `data.nonce` and applies it to inline `<script>` tags.

**Pro:** No API change to `Bundle#render`. Only the view helper changes.

**Con:** Every JS bundle must explicitly handle nonce propagation.

### Option B: Post-process HTML output

After rendering, parse HTML and inject `nonce` attribute into all inline `<script>` tags before returning.

**Pro:** Zero JS-side changes. Works with any framework.

**Con:** HTML parsing overhead. Fragile regex-based injection.

### Option C: Template handler injection

Create a `.ssr` template handler that wraps SSR output with CSP-aware layout.

**Pro:** Clean separation. No changes to existing helpers.

**Con:** Requires new template handler. Deferred feature.

---

## Implementation

### View Helper change

```ruby
def ssr_render(data = nil, **options)
  nonce = options.delete(:nonce) || content_security_policy_nonce(request)
  bundle_name = options.delete(:bundle) || :application
  bundle = find_bundle!(bundle_name)
  html = bundle.render(data, **options)
  html = inject_nonce(html, nonce) if nonce
  html.html_safe
end
```

### Nonce injection

```
── TBD: Decide Option A vs B vs C
```

---

## Files changed

| File | Change |
|------|--------|
| `lib/ssr/deno/rails/helper.rb` | Add `nonce:` option, injection logic |
| `test/ssr/rails/helper_test.rb` | Add nonce test cases |

---

## Verification

1. Bundle renders without nonce when none provided
2. Inline `<script>` tags get nonce when `nonce:` provided
3. CSP header + nonce = no CSP violation in browser
4. Existing tests still pass
