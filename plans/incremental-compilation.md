# Incremental Compilation Investigation (Resolved)

## Summary

Two bugs were found and fixed:

1. **`source_pattern` too narrow**: `'*.rs'` only matched `.rs` files directly in `ext/ssr_deno/`, missing `src/*.rs`. Fixed by adding `ext.extra_sources = FileList['ext/ssr_deno/src/*.rs']`.

2. **Guard `.dup` bug**: `platform_task.prerequisites` returns the internal array by reference, so `clear_prerequisites` mutated the same array captured in `old_prereqs`, causing the `copy:...` prerequisite to be lost. Fixed by using `.dup`.

## Verification

- Modified `ext/ssr_deno/src/lib.rs` → `./bin/compile` → cargo recompiled only `ssr_deno` crate (incremental, 3m 00s vs 3m 31s full) → `.so` installed to `lib/ssr/deno/` successfully.
- All 3 tests pass (14 assertions, 0 failures, 0 errors).
- Rubocop: 9 files inspected, 0 offenses.

## Problem

When a Rust source file (e.g., `ext/ssr_deno/src/lib.rs`) is modified and `./bin/compile` is re-run, cargo does rebuild the `.so` in `tmp/x86_64-linux/ssr_deno/4.0.3/`, but the final `lib/ssr/deno/ssr_deno.so` is **not** updated. The copy step from `tmp/` to `lib/` is skipped.

## Build Pipeline

```
./bin/compile
  └─ bundle exec rake compile
       └─ compile:x86_64-linux
            └─ compile:ssr_deno:x86_64-linux  (our guard is here)
                 └─ copy:ssr_deno:x86_64-linux:4.0.3
                      ├─ lib_path          (directory task)
                      ├─ tmp_binary_path   (file task)
                      └─ tmp_path/Makefile (file task)
```

### The `copy` task (line 163 of extensiontask.rb)

```ruby
task "copy:#{@name}:#{platf}:#{ruby_ver}" => [lib_path, tmp_binary_path, "#{tmp_path}/Makefile"] do
  # runs: make install sitearchdir=relative_lib_path ...
end
```

This is a **regular task** (not a `file` task), so it always runs when invoked. But it's only invoked as a prerequisite of `compile:ssr_deno:x86_64-linux`.

### The `tmp_binary_path` file task (line 187)

```ruby
file tmp_binary_path => [tmp_binary_dir_path, "#{tmp_path}/Makefile"] + source_files do
  chdir tmp_path do
    sh make
  end
end
```

This is a **file task**. Rake's file task invokes the body only if the target file doesn't exist or any prerequisite is newer.

### The `lib_binary_path` file task (line 238)

```ruby
file lib_binary_path => ["copy:#{name}:#{platf}:#{ruby_ver}"]
```

This makes `lib/ssr/deno/ssr_deno.so` depend on the `copy` task. Since `copy` is a regular task (not a file task), this dependency should always trigger the `copy` task.

## Root Cause Analysis

### Key insight from the Makefile (line 130-132 of `create_rust_makefile`)

```makefile
$(RUSTLIB): FORCE
	$(ECHO) generating $(@) ...
	$(full_cargo_command)
```

The `$(RUSTLIB)` target has `FORCE` as a prerequisite, so `make` always runs cargo. This is correct — cargo handles its own incremental compilation internally.

### The `install-so` target (line 138-142)

```makefile
install-so: $(DLLIB) $(timestamp_file("sitearchdir"))
	$(ECHO) installing $(DLLIB) to $(RUBYARCHDIR)
	$(INSTALL_PROG) $(DLLIB) $(RUBYARCHDIR)
```

And `$(DLLIB)` depends on `$(RUSTLIB)` (line 134):
```makefile
$(DLLIB): $(RUSTLIB)
	$(Q) $(COPY) "$(RUSTLIB)" $@
```

So `make install-so` always copies from `$(RUSTLIB)` to `$(DLLIB)` because `$(RUSTLIB)` has `FORCE`.

### The `install` target (line 148)

```makefile
install: #{builder.clean_after_install ? "gemclean" : "install-so"}
```

Since `clean_after_install` is `false` (not running via `gem install`), `install` depends on `install-so`.

### So `make install` should always work...

The Makefile logic is correct — `make install` always triggers cargo (via FORCE), copies the result to `$(DLLIB)`, and installs to `$(RUBYARCHDIR)`.

### The real issue: Rake's file task for `tmp_binary_path`

The problem is in the **Rake layer**, not the Makefile layer. Here's the chain:

1. `compile:ssr_deno:x86_64-linux` → depends on `copy:ssr_deno:x86_64-linux:4.0.3`
2. `copy:ssr_deno:x86_64-linux:4.0.3` → depends on `tmp_binary_path` (file task)
3. `tmp_binary_path` → depends on `"#{tmp_path}/Makefile"` + `source_files`

**The `tmp_binary_path` file task** checks if its target (the `.so` in `tmp/`) is newer than its prerequisites. If the `.so` already exists and is newer than all prerequisites, **the task body is NOT executed**.

But wait — the `tmp_binary_path` task body runs `sh make`, which triggers `make install`... No, actually it just runs `sh make` (line 195), which builds `$(DLLIB)` (the default target, line 150: `all: #{$extout ? "install" : "$(DLLIB)"}`).

Since `$extout` is nil/false, `all` depends on `$(DLLIB)`, which depends on `$(RUSTLIB)` (FORCE). So `make` always runs cargo and copies to `$(DLLIB)`.

**But the `copy` task runs `make install`** (line 168-174), which installs to `$(RUBYARCHDIR)` = `lib/ssr/deno/`.

### The actual flow when `tmp_binary_path` is up-to-date

If `tmp_binary_path` file task considers itself up-to-date (`.so` exists and is newer than source files), the task body (`sh make`) is **skipped**. But the `copy` task still runs because it's a regular task.

Wait — actually, let me re-read the code. The `copy` task is:

```ruby
task "copy:#{@name}:#{platf}:#{ruby_ver}" => [lib_path, tmp_binary_path, "#{tmp_path}/Makefile"] do
```

This is a **regular task** with `tmp_binary_path` as a prerequisite. For regular tasks, prerequisites are invoked (if they're file tasks, they check timestamps). If `tmp_binary_path` is up-to-date, its body is skipped, but the `copy` task body still runs.

**So the `copy` task SHOULD still run `make install`.** Unless...

### The `lib_binary_path` file task (line 238)

```ruby
file lib_binary_path => ["copy:#{name}:#{platf}:#{ruby_ver}"]
```

This makes `lib/ssr/deno/ssr_deno.so` depend on the `copy` task. But this is a **file task** — it checks if `lib_binary_path` is newer than its prerequisites. Since `"copy:..."` is a string (not a file task), Rake treats it as a regular task prerequisite. For regular task prerequisites, the task is invoked, but the file task still checks timestamps.

**Here's the critical issue**: Rake's file task timestamp comparison for non-file prerequisites. When a file task has a regular task as a prerequisite, Rake invokes the regular task, but for the timestamp comparison, it uses the **current time** as the "timestamp" of the regular task. So `lib_binary_path` should be considered out-of-date if its modification time is before the current time.

Actually, let me check Rake's behavior more carefully. In Rake's source:

```ruby
# From Rake::FileTask
def needed?
  !File.exist?(name) || out_of_date?(timestamp) || application.options.build_all
end

def timestamp
  if File.exist?(name)
    File.mtime(name)
  else
    Rake::LATE
  end
end
```

And for `out_of_date?`:
```ruby
def out_of_date?(stamp)
  @prerequisites.any? { |pn| application[pn, @scope].timestamp > stamp }
end
```

For a regular task (like `copy:...`), `timestamp` returns `Rake::EARLY` (the earliest possible time). So `Rake::EARLY > File.mtime(lib_binary_path)` is `false`, meaning the file task considers itself **up-to-date**!

**This is the root cause!** The `lib_binary_path` file task (line 238) has `copy:...` as a prerequisite. Since `copy:...` is a regular task, its timestamp is `Rake::EARLY`, which is never greater than the existing `.so`'s mtime. So the file task never triggers.

BUT — the `copy` task IS still invoked as a prerequisite of `compile:ssr_deno:x86_64-linux` (line 231). So `make install` should still run...

Wait, let me re-check. The task chain is:

```
compile:ssr_deno:x86_64-linux  (regular task)
  └─ copy:ssr_deno:x86_64-linux:4.0.3  (regular task)
       ├─ lib_path  (directory task)
       ├─ tmp_binary_path  (file task)
       └─ tmp_path/Makefile  (file task)
```

`compile:ssr_deno:x86_64-linux` is a regular task (line 231: `task "compile:#{@name}:#{platf}" => ["copy:#{@name}:#{platf}:#{ruby_ver}"]`). Regular tasks always invoke their prerequisites.

So `copy:...` IS invoked. And `copy:...` has three prerequisites:
1. `lib_path` — directory task, always "up-to-date" if dir exists
2. `tmp_binary_path` — file task, checks timestamps
3. `"#{tmp_path}/Makefile"` — file task, checks timestamps

If `tmp_binary_path` is up-to-date (`.so` exists and is newer than source files), its body is skipped. But the `copy` task body still runs, which calls `make install`.

**So `make install` SHOULD run.** And `make install` always runs cargo (via FORCE) and installs to `lib/`.

### The actual bug

Let me re-examine the timestamps from last night:

- `lib/ssr/deno/ssr_deno.so` = 00:12
- `tmp/.../ssr_deno.so` = 01:10
- `ext/ssr_deno/src/lib.rs` = 00:53
- `tmp/.../Makefile` = 00:58

The `tmp/.../ssr_deno.so` was rebuilt at 01:10 (after `lib.rs` change at 00:53). But `lib/ssr/deno/ssr_deno.so` stayed at 00:12.

This means `make install` either:
1. Didn't run at all, OR
2. Ran but installed the wrong (old) file

**Hypothesis**: The `tmp_binary_path` file task was considered up-to-date (its `.so` at 00:12 was newer than source files at 00:53... wait, that doesn't work. 00:12 < 00:53).

Actually, the `tmp_binary_path` file task depends on `source_files`. Let me check what `source_files` resolves to.

### What are `source_files`?

Looking at `BaseExtensionTask`:

```ruby
def source_files
  @source_files ||= begin
    Dir.glob(File.join(ext_dir, @source_pattern))
  end
end
```

Where `@source_pattern` defaults to `"*.{c,cc,cpp}"` but we set `ext.source_pattern = '*.rs'` in our Rakefile.

And `ext_dir` is `ext/ssr_deno` (the directory containing `extconf.rb`).

So `source_files` = `Dir.glob('ext/ssr_deno/*.rs')`.

**This only matches `.rs` files directly in `ext/ssr_deno/`, NOT in subdirectories like `ext/ssr_deno/src/`!**

That's the bug! The `source_pattern` is `'*.rs'`, which only matches `ext/ssr_deno/*.rs`. But our Rust source files are in `ext/ssr_deno/src/*.rs`. So changes to `lib.rs`, `deno_runtime_wrapper.rs`, `sys.rs`, `nop_types.rs` are **not detected** by the file task's timestamp check.

## Solution

### Option A: Fix `source_pattern` to include subdirectories

Change the Rakefile to use a recursive glob pattern:

```ruby
Rake::ExtensionTask.new('ssr_deno') do |ext|
  ext.lib_dir = 'lib/ssr/deno'
  ext.source_pattern = '**/*.rs'  # was '*.rs'
end
```

This makes `source_files` include `ext/ssr_deno/src/*.rs`, so the `tmp_binary_path` file task detects changes.

**Pros**: Simple, one-line fix.
**Cons**: None.

### Option B: Force the `tmp_binary_path` task to always run

Remove the file task's timestamp check by making it always invoke `make`:

This is harder to do with rake-compiler's API. We'd need to override the task definition.

**Pros**: More robust (doesn't depend on glob patterns).
**Cons**: More complex, defeats cargo's incremental compilation at the Rake level (though cargo still handles it internally).

### Option C: Touch the `tmp_binary_path` after `make install`

Add a post-install step that touches the `lib/ssr/deno/ssr_deno.so` to match the `tmp/` version.

**Pros**: Ensures consistency.
**Cons**: Doesn't fix the root cause; the `copy` task would still skip `make install` if `tmp_binary_path` is up-to-date.

## Recommendation

**Option A** is the simplest and most correct fix. The `source_pattern` should be `'**/*.rs'` to match all Rust source files in subdirectories.

However, we should also verify that this is indeed the issue by checking what `source_files` currently returns.

## Verification Steps

1. Check current `source_files` value by running `ruby -e 'puts Dir.glob("ext/ssr_deno/*.rs").inspect'`
2. Apply Option A fix
3. Modify a Rust source file in `src/`
4. Run `./bin/compile`
5. Verify `lib/ssr/deno/ssr_deno.so` timestamp updates
6. Run tests
