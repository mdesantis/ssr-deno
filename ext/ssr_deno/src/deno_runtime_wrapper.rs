use deno_core::{v8, JsRuntime, RuntimeOptions};
use std::cell::UnsafeCell;

/// Wraps a Tokio runtime and a `deno_core::JsRuntime` (V8 isolate) for SSR.
///
/// The Vite SSR bundle is loaded and evaluated once at initialization.
/// Each call to `block_on_render` extracts the `render` function from the
/// V8 global scope, calls it with JSON-serialized arguments, and returns
/// the rendered HTML string.
///
/// # Safety
///
/// `JsRuntime` is not `Send` or `Sync` by default. However, since Ruby's GVL
/// ensures that only one thread accesses this struct at a time, it is safe to
/// implement these traits. The Tokio runtime is `Send` + `Sync`.
///
/// We use `UnsafeCell` for the `JsRuntime` field to allow interior mutability
/// through an immutable reference. This is safe because Ruby's GVL serializes
/// all access, ensuring no concurrent mutable accesses occur.
pub struct DenoRuntimeWrapper {
    tokio_rt: tokio::runtime::Runtime,
    js_runtime: UnsafeCell<JsRuntime>,
}

// SAFETY: Ruby's GVL serializes all access to this struct. The Tokio runtime
// is only used for `block_on` which is called from the single Ruby thread.
unsafe impl Send for DenoRuntimeWrapper {}
unsafe impl Sync for DenoRuntimeWrapper {}

impl DenoRuntimeWrapper {
    /// Creates a new `DenoRuntimeWrapper`, loading and evaluating the SSR bundle.
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
        let tokio_rt = tokio::runtime::Runtime::new()?;
        let mut js_runtime = JsRuntime::new(RuntimeOptions::default());
        let bundle = std::fs::read_to_string(bundle_path)?;

        // Evaluate the self-contained SSR bundle.
        // This registers the `render` function in the V8 global scope.
        js_runtime.execute_script("entry-server", bundle)?;

        Ok(Self {
            tokio_rt,
            js_runtime: UnsafeCell::new(js_runtime),
        })
    }

    /// Returns a mutable pointer to the inner `JsRuntime`.
    ///
    /// # Safety
    ///
    /// Caller must ensure that no other mutable reference exists concurrently.
    /// Ruby's GVL guarantees this for single-threaded access.
    #[inline]
    fn js_runtime_mut(&self) -> &mut JsRuntime {
        // SAFETY: Ruby's GVL ensures single-threaded access, so getting a
        // mutable reference from an immutable one is safe here.
        unsafe { &mut *self.js_runtime.get() }
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
        let js_runtime = self.js_runtime_mut();

        // Get the main context handle BEFORE creating the scope,
        // to avoid conflicting borrows on js_runtime.
        let main_context = js_runtime.main_context();

        // Get the V8 isolate from the JsRuntime
        let isolate = js_runtime.v8_isolate();

        // Create a HandleScope (pinned to the stack) from the isolate
        v8::scope!(let scope, isolate);

        // Enter the main context
        let context = v8::Local::new(&scope, main_context);
        let scope = v8::ContextScope::new(scope, context);

        // Get the render function from the V8 global scope
        let global = scope.get_current_context().global(&scope);
        let render_key = v8::String::new(&scope, "render").unwrap();
        let render_value = global.get(&scope, render_key.into());

        let render_fn: v8::Local<v8::Function> = match render_value {
            Some(val) => val.try_into().map_err(|_| {
                format!(
                    "render function not found in bundle. \
                     Ensure the bundle exports a function named 'render'."
                )
            })?,
            None => {
                return Err("render function not found in bundle. \
                     Ensure the bundle exports a function named 'render'."
                    .into())
            }
        };

        // Create the JSON argument as a V8 string
        let json_arg = v8::String::new(&scope, args_json).unwrap();
        let undefined = v8::undefined(&scope);

        // Call render(undefined, args_json) — first arg is `this`, second is the JSON string
        let result = render_fn
            .call(&scope, undefined.into(), &[json_arg.into()])
            .ok_or_else(|| "JavaScript render function threw an error".to_string())?;

        let html = result
            .to_string(&scope)
            .ok_or_else(|| "Render result could not be converted to a string".to_string())?
            .to_rust_string_lossy(&scope);

        Ok(html)
    }
}
