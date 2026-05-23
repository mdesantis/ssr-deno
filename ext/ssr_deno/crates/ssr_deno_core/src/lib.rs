//! Pure-Rust types and functions extracted from the `ssr_deno` native extension.
//!
//! This crate has **zero** dependencies on `v8`, `deno_runtime`, `tokio`, or
//! `magnus`, making it fast to compile and testable with plain
//! `cargo test -p ssr_deno_core`.
//!
//! The main `ssr_deno` crate depends on this crate and re-exports or delegates
//! to these types.

use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

pub mod source_mapper;

// ---------------------------------------------------------------------------
// Typed error enum
// ---------------------------------------------------------------------------

/// Errors that can originate from the Deno runtime wrapper layer.
#[derive(Debug)]
pub enum SSRDenoError {
    BundleLoad(String),
    WorkerInit(String),
    WorkerDied(String),
    BundleNotFound(String),
    Render(String),
    OutOfMemory(String),
    /// Heap statistics serialization failed. Should never occur in practice
    /// (the struct contains only plain integers), but distinct from Render to
    /// avoid misleading exception types in Ruby.
    HeapStatsSerialization(String),
}

impl std::fmt::Display for SSRDenoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BundleLoad(msg) => write!(f, "BundleLoad: {msg}"),
            Self::WorkerInit(msg) => write!(f, "WorkerInit: {msg}"),
            Self::WorkerDied(msg) => write!(f, "WorkerDied: {msg}"),
            Self::BundleNotFound(msg) => write!(f, "BundleNotFound: {msg}"),
            Self::Render(msg) => write!(f, "Render: {msg}"),
            Self::OutOfMemory(msg) => write!(f, "OutOfMemory: {msg}"),
            Self::HeapStatsSerialization(msg) => write!(f, "HeapStatsSerialization: {msg}"),
        }
    }
}

impl std::error::Error for SSRDenoError {}

// ---------------------------------------------------------------------------
// Configuration data
// ---------------------------------------------------------------------------

/// Configuration passed from Ruby to Rust before runtime initialization.
/// Defaults are safe for unconfigured usage.
#[derive(Clone, Copy)]
pub struct Config {
    pub max_heap_size_mb: usize,
    pub isolate_pool_size: usize,
    pub render_timeout_ms: u64,
    /// Enable Node.js built-in module support (stream, buffer, events, etc.).
    /// Required for packages like @emotion/server that depend on Node.js
    /// built-in modules via require(). Disabled by default since most SSR
    /// bundles don't need it and it adds worker init overhead.
    pub node_builtins: bool,
    /// Enable source map resolution for V8 error stack traces.
    /// When enabled, the gem reads `.js.map` sidecars and resolves bundle
    /// positions to original `.tsx`/`.ts` source locations.
    pub source_maps: bool,
}

impl Config {
    /// Returns the default configuration: 64 MB heap, 1 isolate pool,
    /// 500ms render timeout, no Node.js builtins, no source maps.
    pub const fn default() -> Self {
        Self {
            max_heap_size_mb: 64,
            isolate_pool_size: 1,
            render_timeout_ms: 500,
            node_builtins: false,
            source_maps: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Pool size validation
// ---------------------------------------------------------------------------

/// Validates that `size` is at least 1.
/// Returns `Ok(())` if valid, or an `SSRDenoError::WorkerInit` error.
pub fn validate_pool_size(size: usize) -> Result<(), SSRDenoError> {
    if size == 0 {
        return Err(SSRDenoError::WorkerInit(
            "Pool size must be at least 1".into(),
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Render timeout validation
// ---------------------------------------------------------------------------

/// Validates that `ms` is within `[100, 300_000]`.
pub fn validate_render_timeout_ms(ms: u64) -> Result<(), String> {
    if ms < 100 {
        Err("Render timeout must be at least 100ms".into())
    } else if ms > 300_000 {
        Err("Render timeout must not exceed 300000ms (5min)".into())
    } else {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Pool size resolution
// ---------------------------------------------------------------------------

/// Resolves the effective pool size from config, clamped to at least 1.
pub fn resolve_pool_size(cfg: Config) -> usize {
    cfg.isolate_pool_size.max(1)
}

// ---------------------------------------------------------------------------
// Round-robin counter
// ---------------------------------------------------------------------------

/// Atomically fetches and increments a round-robin counter, returning the index
/// modulo `len`. Safe to call concurrently from multiple threads.
///
/// # Panics
///
/// Panics if `len == 0`.
pub fn next_index(counter: &AtomicUsize, len: usize) -> usize {
    counter.fetch_add(1, Ordering::Relaxed) % len
}

// ---------------------------------------------------------------------------
// Heap size overflow check
// ---------------------------------------------------------------------------

/// Validates that `mb * 1024 * 1024` doesn't overflow `usize`.
///
/// Returns `Ok(mb * 1024 * 1024)` on success, or an error message on overflow.
///
/// On 64-bit: max ≈ 16,384,000 MB (16 TB).
/// On 32-bit: max ≈ 4,096 MB.
pub fn max_heap_size_mb_checked(mb: usize) -> Result<usize, &'static str> {
    mb.checked_mul(1024 * 1024)
        .ok_or("max_heap_size_mb overflows when converted to bytes")
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests;
