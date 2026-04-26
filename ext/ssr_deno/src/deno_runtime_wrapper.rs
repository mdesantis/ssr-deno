use std::cell::UnsafeCell;

/// Wraps a Tokio runtime and a `deno_runtime::MainWorker` (V8 isolate with Deno
/// Web API extensions) for SSR.
///
/// The Vite SSR bundle is loaded and evaluated once at initialization.
/// Each call to `block_on_render` extracts the `render` function from the
/// V8 global scope, calls it with JSON-serialized arguments, and returns
/// the rendered HTML string.
///
/// # Web API Support
///
/// `MainWorker` provides all Deno Web API extensions out of the box:
/// - `MessageChannel` / `MessagePort` (React 19 scheduler)
/// - `setTimeout` / `clearTimeout` / `setInterval` / `clearInterval`
/// - `performance.now()`
/// - `console`
/// - `TextEncoder` / `TextDecoder`
/// - `URL`, `Blob`, `FormData`, `Headers`
/// - `fetch`, `WebSocket`, `crypto`, and more
///
/// # Safety
///
/// `MainWorker` is not `Send` or `Sync` by default. However, since Ruby's GVL
/// ensures that only one thread accesses this struct at a time, it is safe to
/// implement these traits. The Tokio runtime is `Send` + `Sync`.
///
/// We use `UnsafeCell` for the `MainWorker` field to allow interior mutability
/// through an immutable reference. This is safe because Ruby's GVL serializes
/// all access, ensuring no concurrent mutable accesses occur.
pub struct DenoRuntimeWrapper {
    tokio_rt: tokio::runtime::Runtime,
    worker: UnsafeCell<deno_runtime::worker::MainWorker>,
}

// SAFETY: Ruby's GVL serializes all access to this struct. The Tokio runtime
// is only used for `block_on` which is called from the single Ruby thread.
unsafe impl Send for DenoRuntimeWrapper {}
unsafe impl Sync for DenoRuntimeWrapper {}

impl DenoRuntimeWrapper {
    /// Creates a new `DenoRuntimeWrapper`, loading and evaluating the SSR bundle.
    ///
    /// Initializes a `MainWorker` with all Deno Web API extensions and evaluates
    /// the self-contained Vite SSR bundle.
    ///
    /// # Arguments
    ///
    /// * `bundle_path` - Path to the self-contained Vite SSR bundle (entry-server.js)
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The bundle file cannot be read
    /// - The bundle JavaScript cannot be evaluated (syntax error, runtime error)
    pub fn new(bundle_path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        todo!("implement MainWorker initialization")
    }

    /// Returns a mutable pointer to the inner `MainWorker`.
    ///
    /// # Safety
    ///
    /// Caller must ensure that no other mutable reference exists concurrently.
    /// Ruby's GVL guarantees this for single-threaded access.
    #[inline]
    fn worker_mut(&self) -> &mut deno_runtime::worker::MainWorker {
        // SAFETY: Ruby's GVL ensures single-threaded access, so getting a
        // mutable reference from an immutable one is safe here.
        unsafe { &mut *self.worker.get() }
    }

    /// Calls the `render` function from the evaluated SSR bundle with JSON args.
    ///
    /// # Arguments
    ///
    /// * `args_json` - JSON string containing `{ component_data, props, url }`
    ///
    /// # Returns
    ///
    /// The rendered HTML string from the SSR bundle.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The `render` function is not found in the global scope
    /// - The JavaScript `render` function throws an error
    /// - The V8 value cannot be converted to a string
    pub fn block_on_render(&self, args_json: &str) -> Result<String, Box<dyn std::error::Error>> {
        todo!("implement render via MainWorker.js_runtime")
    }
}
