# Split `mod.rs` — Deno Runtime Wrapper

_target: `ext/ssr_deno/src/deno_runtime_wrapper/`_

`mod.rs` is 773 lines. 6 sections → 5 new files + thin `mod.rs`.

## Current layout (lines)

| Lines | Content | Destination |
|-------|---------|-------------|
| 1-32 | Imports + `ChunkedRenderResult` | split per file |
| 34-41 | Module declarations (`heap_stats`, `render`, etc.) | stay in `mod.rs` |
| 43-75 | `intern_script_name` | stay in `mod.rs` |
| 77-105 | `WorkerMsg` enum | `types.rs` |
| 107-221 | `IsolateHandle` struct + impl | `handle.rs` |
| 223-375 | `IsolatePool` struct + impl | `pool.rs` |
| 377-492 | `worker_thread_main` | `worker.rs` |
| 494-564 | `setup_require` | `worker.rs` |
| 566-620 | `load_bundle_in_worker` | `worker.rs` |
| 622-674 | `build_node_services` | `builder.rs` |
| 676-773 | `build_worker` | `builder.rs` |

## Visibility rules

- New modules: `pub(crate) mod` so siblings can reference each other
- `WorkerMsg`, `ChunkedRenderResult`: `pub(crate)` in `types.rs` (was private in `mod.rs`, siblings need access)
- `IsolateHandle`: keep `pub` (unchanged, though only used internally)
- `IsolatePool`: keep `pub`, add `pub use pool::IsolatePool;` in `mod.rs`
- `SSRDenoError`, `next_index`, `validate_pool_size`: `pub use` stays in `mod.rs`

## File-by-file

### `types.rs` — ~28 lines

```rust
use tokio::sync::{mpsc, oneshot};

pub(crate) type ChunkedRenderResult = (
    mpsc::Receiver<String>,
    oneshot::Receiver<Result<(), ssr_deno_core::SSRDenoError>>,
);

pub(crate) enum WorkerMsg {
    LoadBundle { bundle_id: String, bundle_path: String, bundle_code: std::sync::Arc<str>, script_name: &'static str, reply: oneshot::Sender<Result<(), String>> },
    Render { bundle_id: String, args_json: String, render_timeout_ms: u64, reply: oneshot::Sender<Result<String, ssr_deno_core::SSRDenoError>> },
    RenderChunked { bundle_id: String, args_json: String, render_timeout_ms: u64, chunk_tx: mpsc::Sender<String>, reply: oneshot::Sender<Result<(), ssr_deno_core::SSRDenoError>> },
    HeapStats { reply: oneshot::Sender<Result<String, ssr_deno_core::SSRDenoError>> },
}
```

Import `SSRDenoError` from `ssr_deno_core` directly (crate-level dep), not via `super::`. Avoids circular concern: `types.rs` is the lowest layer.

### `handle.rs` — ~115 lines

```rust
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;

use tokio::sync::{mpsc, oneshot};

use super::types::{ChunkedRenderResult, WorkerMsg};
use super::SSRDenoError;
```

- `IsolateHandle` struct with `tx: tokio::sync::mpsc::Sender<WorkerMsg>`, `render_timeout_ms: u64`
- Methods: `spawn`, `block_on_render`, `start_render_chunked`, `block_on_heap_stats`, `blocking_send`
- `spawn` uses `std::thread::Builder::new()` and `mpsc::sync_channel` for init sync

### `pool.rs` — ~150 lines

```rust
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use tokio::sync::oneshot;

use super::handle::IsolateHandle;
use super::types::{ChunkedRenderResult, WorkerMsg};
use super::{intern_script_name, next_index, validate_pool_size, SSRDenoError};
```

- `IsolatePool` struct with `handles: Vec<IsolateHandle>`, `counter: AtomicUsize`
- Methods: `new`, `size`, `next_handle`, `dispatch_render`, `dispatch_render_chunked`, `heap_stats`, `load_bundle`
- `load_bundle` calls `super::intern_script_name` (still lives in `mod.rs`)
- `next_handle` calls `super::next_index`
- `new` calls `super::validate_pool_size`

### `worker.rs` — ~240 lines

```rust
use std::collections::HashSet;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

use deno_runtime::deno_core::url::Url;
use deno_runtime::deno_core::v8;
use tokio::runtime;
use tokio::task::LocalSet;

use super::builder::build_worker;
use super::heap_stats::collect_heap_stats;
use super::render;
use super::render_chunked;
use super::types::WorkerMsg;
```

- `worker_thread_main` — the per-isolate event loop, processes `WorkerMsg` variants
- `setup_require` — injects `globalThis.require` via `import('node:module')`
- `load_bundle_in_worker` — evaluates bundle code, registers in `__ssr_bundles`
- `loaded_paths: HashSet<(String, String)>` stays local to `worker_thread_main`

### `builder.rs` — ~150 lines

```rust
use std::borrow::Cow;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use deno_runtime::deno_core::v8;
use deno_runtime::deno_core::url::Url;
use deno_runtime::deno_fs::sync::MaybeArc;
use deno_runtime::deno_node::{NodeRequireLoaderRc, NodeResolver};
use deno_runtime::deno_permissions::{Permissions, PermissionsContainer};
use deno_runtime::worker::{MainWorker, WorkerOptions, WorkerServiceOptions};
use deno_runtime::BootstrapOptions;
use deno_runtime::FeatureChecker;
use node_resolver::cache::NodeResolutionSys;
use node_resolver::DenoIsBuiltInNodeModuleChecker;
use node_resolver::{NodeConditionOptions, NodeResolverOptions, PackageJsonResolver};

use crate::node_builtin_loader::NodeBuiltinOnlyModuleLoader;
use crate::nop_types::{NopInNpmPackageChecker, NopNpmPackageFolderResolver, NopPermissionDescriptorParser};
use crate::require_loader::SSRDenoNodeRequireLoader;
use crate::sys::Sys;

use super::SSRDenoError;
```

- `type NodeServices = ...` — private type alias
- `build_node_services(node_builtins: bool) -> Option<NodeServices>`
- `build_worker(...) -> Result<MainWorker, String>` — also registers near-heap-limit callback

### `mod.rs` (thin) — ~50 lines

```rust
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

pub use ssr_deno_core::SSRDenoError;
pub use ssr_deno_core::{next_index, validate_pool_size};

pub(crate) mod heap_stats;
pub(crate) mod render;
pub(crate) mod render_chunked;
pub(crate) mod watchdog;

pub(crate) mod types;
pub(crate) mod handle;
pub(crate) mod pool;
pub(crate) mod worker;
pub(crate) mod builder;

pub use pool::IsolatePool;

/// Script name interning...
static SCRIPT_NAMES: OnceLock<Mutex<HashMap<String, &'static str>>> = OnceLock::new();

fn intern_script_name(name: &str) -> &'static str { ... }
```

## Cross-submodule references (unchanged)

| From | Referencing | Still works |
|------|-------------|-------------|
| `render.rs` | `super::watchdog::Watchdog` | ✓ |
| `render.rs` | `super::SSRDenoError` | ✓ re-exported from mod.rs |
| `render_chunked.rs` | `super::render::{begin_render, …}` | ✓ |
| `render_chunked.rs` | `super::SSRDenoError` | ✓ |
| `heap_stats.rs` | `super::SSRDenoError` | ✓ |

## External API

`lib.rs` imports `{IsolatePool, SSRDenoError}`. Both available:
- `SSRDenoError` — `pub use` in `mod.rs` (unchanged)
- `IsolatePool` — `pub use pool::IsolatePool;` in `mod.rs` (new re-export)

## Migration order

1. Create `types.rs` — no deps on other new files
2. Create `builder.rs` — deps on `types` via `super::SSRDenoError`
3. Create `handle.rs` — deps on `types`
4. Create `worker.rs` — deps on `builder`, `types`, existing submodules
5. Create `pool.rs` — deps on `handle`, `types`, `mod.rs` (intern_script_name)
6. Thin down `mod.rs` — remove moved code, add re-exports
7. `cargo test -p ssr_deno_core` + `cargo clippy` + `cargo fmt`
8. `bundle exec rake` (full pipeline)

## Post-implementation stale audit

After the split, run an exceptionally thorough search for stale content:

- **Module path comments** — any doc/comment referencing old functions/types by their `mod.rs` location (e.g. "see `mod.rs:120`" or "defined in `mod.rs`"). These need updating to the new file path.
- **`use crate::deno_runtime_wrapper` imports** — check no path broke (`lib.rs` only, but verify).
- **Plans** — `plans/archived/rust-future-work.md` references `mod.rs` line numbers for the future items. Update those.
- **AGENTS.md** — any mention of `mod.rs` line numbers or file structure.
- **CHANGELOG.md** — verify no stale path references.
- **Source comments inside the moved code blocks** — any comment that says "see mod.rs" or references relative position within `mod.rs` will be misleading in its new file.

Command to catch stale line-number refs:
```
rg 'mod\.rs:\d+' ext/ssr_deno/src/ plans/ AGENTS.md CHANGELOG.md
```

The `mod.rs` line numbers WILL shift after the split, so every `mod.rs:N` reference in the moved code and in external docs must be reviewed.
