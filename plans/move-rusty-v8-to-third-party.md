# Plan: Move `ext/ssr_deno/rusty_v8` to `third_party/rusty_v8`

## Rationale

The `rusty_v8` git submodule is currently nested inside `ext/ssr_deno/`, which is the Rust native extension directory. Moving it to a root-level `third_party/` folder improves project organization by:

- Separating vendored/third-party code from first-party source code
- Following common conventions (e.g., Chromium, many C++/Rust projects use `third_party/`)
- Making it clear that `rusty_v8` is a patched dependency, not part of the extension's own source

## Files Requiring Changes

### 1. `.gitmodules` — Submodule definition
| Current | New |
|---------|-----|
| `[submodule "ext/ssr_deno/rusty_v8"]` | `[submodule "third_party/rusty_v8"]` |
| `path = ext/ssr_deno/rusty_v8` | `path = third_party/rusty_v8` |

### 2. `.git/config` — Local git config
| Current | New |
|---------|-----|
| `[submodule "ext/ssr_deno/rusty_v8"]` | `[submodule "third_party/rusty_v8"]` |

### 3. `ext/ssr_deno/Cargo.toml` — Cargo patch override (line 26)
| Current | New |
|---------|-----|
| `v8 = { path = "rusty_v8" }` | `v8 = { path = "../../third_party/rusty_v8" }` |

Relative path from `ext/ssr_deno/` → `third_party/rusty_v8` = `../../third_party/rusty_v8`

### 4. `.rubocop.yml` — RuboCop exclude pattern (line 11)
| Current | New |
|---------|-----|
| `- 'ext/ssr_deno/rusty_v8/**/*'` | `- 'third_party/rusty_v8/**/*'` |

### 5. `.vscode/settings.json` — rust-analyzer excludeDirs (line 21)
| Current | New |
|---------|-----|
| `"ext/ssr_deno/rusty_v8"` | `"third_party/rusty_v8"` |

### 6. `plans/v8-tls-issue.md` — Documentation (line 43)
Update the path reference in the documentation comment. The text currently says:
> The `[patch.crates-io]` section in [`ext/ssr_deno/Cargo.toml`](../ext/ssr_deno/Cargo.toml) points to the local checkout.

This is a documentation-only change and doesn't affect functionality. The relative link `../ext/ssr_deno/Cargo.toml` is from `plans/` directory and remains correct since `Cargo.toml` isn't moving.

### 7. `.gitignore` — No change needed
The submodule is tracked by git, not ignored. No `.gitignore` changes required.

### 8. `.env.example` — No change needed
References `librusty_v8.a` as a library name, not a path. No change required.

### 9. `plans/architecture.md` — No change needed
References `rusty_v8` as a crate name, not a filesystem path. No change required.

## Execution Steps

### Step 1: Deinit the submodule
```bash
git submodule deinit -f ext/ssr_deno/rusty_v8
```

### Step 2: Remove the old directory
```bash
rm -rf ext/ssr_deno/rusty_v8
```

### Step 3: Create the new directory
```bash
mkdir -p third_party
```

### Step 4: Update `.gitmodules`
Change the submodule name and path from `ext/ssr_deno/rusty_v8` to `third_party/rusty_v8`.

### Step 5: Update `.git/config`
Change the submodule section header from `[submodule "ext/ssr_deno/rusty_v8"]` to `[submodule "third_party/rusty_v8"]`.

### Step 6: Re-init the submodule at the new path
```bash
git submodule update --init --recursive
```
This will clone the submodule into `third_party/rusty_v8`.

### Step 7: Update `ext/ssr_deno/Cargo.toml`
Change the `[patch.crates-io]` path from `"rusty_v8"` to `"../../third_party/rusty_v8"`.

### Step 8: Update `.rubocop.yml`
Change the exclude pattern from `ext/ssr_deno/rusty_v8/**/*` to `third_party/rusty_v8/**/*`.

### Step 9: Update `.vscode/settings.json`
Change the `rust-analyzer.files.excludeDirs` entry from `ext/ssr_deno/rusty_v8` to `third_party/rusty_v8`.

### Step 10: Update `plans/v8-tls-issue.md` (optional, documentation only)
Update the path reference text if desired.

### Step 11: Verify the build
```bash
bundle exec rake compile
```

## Verification Checklist

- [ ] `git submodule status` shows `third_party/rusty_v8` with a valid commit hash
- [ ] `ext/ssr_deno/Cargo.toml` patch path resolves correctly (`../../third_party/rusty_v8`)
- [ ] `bundle exec rake compile` succeeds
- [ ] `bundle exec rake test` passes
- [ ] `git status` shows no unexpected changes

## Rollback Plan

If the build fails after the move:

1. Revert all config file changes (`.gitmodules`, `.git/config`, `Cargo.toml`, `.rubocop.yml`, `.vscode/settings.json`)
2. Deinit the new submodule: `git submodule deinit -f third_party/rusty_v8`
3. Re-init at the old path: `git submodule update --init --recursive`
4. Or simply: `git checkout -- .gitmodules ext/ssr_deno/Cargo.toml .rubocop.yml .vscode/settings.json` followed by `git submodule update --init --recursive`
