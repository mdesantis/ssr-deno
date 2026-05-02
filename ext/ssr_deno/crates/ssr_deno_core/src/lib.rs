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

// ---------------------------------------------------------------------------
// Typed error enum
// ---------------------------------------------------------------------------

/// Errors that can originate from the Deno runtime wrapper layer.
#[derive(Debug)]
pub enum DenoError {
    BundleLoad(String),
    WorkerInit(String),
    WorkerDied(String),
    BundleNotFound(String),
    Render(String),
}

impl std::fmt::Display for DenoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BundleLoad(msg)
            | Self::WorkerInit(msg)
            | Self::WorkerDied(msg)
            | Self::BundleNotFound(msg)
            | Self::Render(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for DenoError {}

// ---------------------------------------------------------------------------
// Hard cap on the number of isolates
// ---------------------------------------------------------------------------

/// Maximum number of V8 isolates in the pool. Prevents accidental
/// over-allocation on high-core-count machines.
pub const MAX_ISOLATES: usize = 8;

// ---------------------------------------------------------------------------
// Configuration data
// ---------------------------------------------------------------------------

/// Configuration passed from Ruby to Rust before runtime initialization.
/// Defaults are safe for unconfigured usage.
#[derive(Clone, Copy)]
pub struct Config {
    pub max_heap_size_mb: usize,
    /// 0 = auto-detect from CPU count
    pub isolate_pool_size: usize,
    pub render_timeout_ms: u64,
    /// Enable Node.js built-in module support (stream, buffer, events, etc.).
    /// Required for packages like @emotion/server that depend on Node.js
    /// built-in modules via require(). Disabled by default since most SSR
    /// bundles don't need it and it adds worker init overhead.
    pub node_builtins: bool,
}

impl Config {
    /// Returns the default configuration: 64 MB heap, auto-detect pool size,
    /// 500ms render timeout, no Node.js builtins.
    pub const fn default() -> Self {
        Self {
            max_heap_size_mb: 64,
            isolate_pool_size: 0,
            render_timeout_ms: 500,
            node_builtins: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Pool size validation
// ---------------------------------------------------------------------------

/// Validates that `size` is within `[1, MAX_ISOLATES]`.
/// Returns `Ok(())` if valid, or an appropriate `DenoError::WorkerInit` error.
pub fn validate_pool_size(size: usize) -> Result<(), DenoError> {
    if size == 0 {
        return Err(DenoError::WorkerInit(
            "Pool size must be at least 1".into(),
        ));
    }
    if size > MAX_ISOLATES {
        return Err(DenoError::WorkerInit(format!(
            "Pool size {size} exceeds maximum {MAX_ISOLATES}"
        )));
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

/// Resolves the effective pool size from config.
/// - `0` (default) → auto-detect from CPU count, capped at `MAX_ISOLATES`
/// - `> 0`         → as-is, capped at `MAX_ISOLATES`
pub fn resolve_pool_size(cfg: Config) -> usize {
    let raw = if cfg.isolate_pool_size > 0 {
        cfg.isolate_pool_size
    } else {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(2)
            .saturating_sub(1) // leave one core for Ruby
    };
    std::cmp::max(1, std::cmp::min(raw, MAX_ISOLATES))
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
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    // -----------------------------------------------------------------------
    // DenoError
    // -----------------------------------------------------------------------

    #[test]
    fn deno_error_display_bundle_load() {
        let e = DenoError::BundleLoad("foo".into());
        assert_eq!(format!("{e}"), "foo");
    }

    #[test]
    fn deno_error_display_worker_init() {
        let e = DenoError::WorkerInit("bar".into());
        assert_eq!(format!("{e}"), "bar");
    }

    #[test]
    fn deno_error_display_worker_died() {
        let e = DenoError::WorkerDied("baz".into());
        assert_eq!(format!("{e}"), "baz");
    }

    #[test]
    fn deno_error_display_bundle_not_found() {
        let e = DenoError::BundleNotFound("qux".into());
        assert_eq!(format!("{e}"), "qux");
    }

    #[test]
    fn deno_error_display_render() {
        let e = DenoError::Render("err".into());
        assert_eq!(format!("{e}"), "err");
    }

    #[test]
    fn deno_error_source_is_none() {
        use std::error::Error;
        let e = DenoError::Render("x".into());
        assert!(e.source().is_none());
    }

    // -----------------------------------------------------------------------
    // MAX_ISOLATES
    // -----------------------------------------------------------------------

    #[test]
    fn max_isolates_is_eight() {
        assert_eq!(MAX_ISOLATES, 8);
    }

    // -----------------------------------------------------------------------
    // Config defaults
    // -----------------------------------------------------------------------

    #[test]
    fn config_default_max_heap() {
        let cfg = Config::default();
        assert_eq!(cfg.max_heap_size_mb, 64);
    }

    #[test]
    fn config_default_pool_size() {
        let cfg = Config::default();
        assert_eq!(cfg.isolate_pool_size, 0);
    }

    #[test]
    fn config_default_render_timeout() {
        let cfg = Config::default();
        assert_eq!(cfg.render_timeout_ms, 500);
    }

    #[test]
    fn config_default_node_builtins() {
        let cfg = Config::default();
        assert!(!cfg.node_builtins);
    }

    // -----------------------------------------------------------------------
    // validate_render_timeout_ms
    // -----------------------------------------------------------------------

    #[test]
    fn validate_render_timeout_accepts_100() {
        assert!(validate_render_timeout_ms(100).is_ok());
    }

    #[test]
    fn validate_render_timeout_rejects_99() {
        assert!(validate_render_timeout_ms(99).is_err());
    }

    #[test]
    fn validate_render_timeout_accepts_300000() {
        assert!(validate_render_timeout_ms(300_000).is_ok());
    }

    #[test]
    fn validate_render_timeout_rejects_300001() {
        assert!(validate_render_timeout_ms(300_001).is_err());
    }

    // -----------------------------------------------------------------------
    // validate_pool_size
    // -----------------------------------------------------------------------

    #[test]
    fn validate_pool_size_rejects_zero() {
        let err = validate_pool_size(0).unwrap_err();
        assert!(matches!(err, DenoError::WorkerInit(_)));
        assert!(format!("{err}").contains("at least 1"));
    }

    #[test]
    fn validate_pool_size_rejects_over_max() {
        let err = validate_pool_size(MAX_ISOLATES + 1).unwrap_err();
        assert!(matches!(err, DenoError::WorkerInit(_)));
        assert!(format!("{err}").contains("exceeds maximum"));
    }

    #[test]
    fn validate_pool_size_accepts_one() {
        assert!(validate_pool_size(1).is_ok());
    }

    #[test]
    fn validate_pool_size_accepts_max() {
        assert!(validate_pool_size(MAX_ISOLATES).is_ok());
    }

    // -----------------------------------------------------------------------
    // resolve_pool_size
    // -----------------------------------------------------------------------

    fn make_cfg(pool_size: usize) -> Config {
        Config {
            isolate_pool_size: pool_size,
            max_heap_size_mb: 64,
            render_timeout_ms: 500,
            node_builtins: false,
        }
    }

    #[test]
    fn resolve_pool_size_uses_explicit_value() {
        assert_eq!(resolve_pool_size(make_cfg(4)), 4);
    }

    #[test]
    fn resolve_pool_size_clamps_to_max() {
        assert_eq!(resolve_pool_size(make_cfg(99)), MAX_ISOLATES);
    }

    #[test]
    fn resolve_pool_size_minimum_is_one() {
        let size = resolve_pool_size(make_cfg(0));
        assert!(size >= 1, "pool size {size} should be >= 1");
        assert!(size <= MAX_ISOLATES, "pool size {size} should be <= {MAX_ISOLATES}");
    }

    #[test]
    fn resolve_pool_size_auto_detect_is_sensible() {
        let size = resolve_pool_size(make_cfg(0));
        assert!(size >= 1);
        assert!(size <= MAX_ISOLATES);
    }

    // -----------------------------------------------------------------------
    // next_index (round-robin)
    // -----------------------------------------------------------------------

    #[test]
    fn next_index_cycles_through_three_slots() {
        let counter = AtomicUsize::new(0);
        let expected = [0, 1, 2, 0, 1, 2];
        for &exp in &expected {
            assert_eq!(next_index(&counter, 3), exp);
        }
    }

    #[test]
    fn next_index_single_slot_always_zero() {
        let counter = AtomicUsize::new(0);
        for _ in 0..10 {
            assert_eq!(next_index(&counter, 1), 0);
        }
    }

    #[test]
    fn next_index_wraps_without_panic() {
        // Start near usize::MAX to verify wrapping doesn't panic.
        // fetch_add returns the OLD value, so each assertion checks the
        // pre-increment value modulo len.
        let counter = AtomicUsize::new(usize::MAX - 1);
        assert_eq!(next_index(&counter, 3), (usize::MAX - 1) % 3);  // returns (MAX-1)%3, counter now MAX
        assert_eq!(next_index(&counter, 3), (usize::MAX) % 3);      // returns MAX%3, counter wraps to 0
        assert_eq!(next_index(&counter, 3), 0);                     // returns 0 (old value), counter now 1
    }

    #[test]
    #[should_panic(expected = "attempt to calculate the remainder with a divisor of zero")]
    fn next_index_panics_on_zero_len() {
        let counter = AtomicUsize::new(0);
        next_index(&counter, 0);
    }

    // -----------------------------------------------------------------------
    // max_heap_size_mb_checked
    // -----------------------------------------------------------------------

    #[test]
    fn heap_checked_zero_ok() {
        assert_eq!(max_heap_size_mb_checked(0).unwrap(), 0);
    }

    #[test]
    fn heap_checked_64_mb() {
        assert_eq!(max_heap_size_mb_checked(64).unwrap(), 64 * 1024 * 1024);
    }

    #[test]
    fn heap_checked_max_boundary_ok() {
        let max_mb = usize::MAX / 1024 / 1024;
        assert!(max_heap_size_mb_checked(max_mb).is_ok());
    }

    #[test]
    fn heap_checked_overflow_errors() {
        let overflow_mb = usize::MAX / 1024 / 1024 + 1;
        let err = max_heap_size_mb_checked(overflow_mb).unwrap_err();
        assert!(err.contains("overflows"));
    }
}
