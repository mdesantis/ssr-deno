use super::*;
use std::sync::atomic::AtomicUsize;

// -----------------------------------------------------------------------
// SSRDenoError
// -----------------------------------------------------------------------

#[test]
fn deno_error_display_bundle_load() {
    let e = SSRDenoError::BundleLoad("foo".into());
    assert_eq!(format!("{e}"), "BundleLoad: foo");
}

#[test]
fn deno_error_display_worker_init() {
    let e = SSRDenoError::WorkerInit("bar".into());
    assert_eq!(format!("{e}"), "WorkerInit: bar");
}

#[test]
fn deno_error_display_worker_died() {
    let e = SSRDenoError::WorkerDied("baz".into());
    assert_eq!(format!("{e}"), "WorkerDied: baz");
}

#[test]
fn deno_error_display_bundle_not_found() {
    let e = SSRDenoError::BundleNotFound("qux".into());
    assert_eq!(format!("{e}"), "BundleNotFound: qux");
}

#[test]
fn deno_error_display_render() {
    let e = SSRDenoError::Render("err".into());
    assert_eq!(format!("{e}"), "Render: err");
}

#[test]
fn deno_error_display_out_of_memory() {
    let e = SSRDenoError::OutOfMemory("oom".into());
    assert_eq!(format!("{e}"), "OutOfMemory: oom");
}

#[test]
fn deno_error_display_heap_stats_serialization() {
    let e = SSRDenoError::HeapStatsSerialization("ser".into());
    assert_eq!(format!("{e}"), "HeapStatsSerialization: ser");
}

#[test]
fn deno_error_source_is_none() {
    use std::error::Error;
    let e = SSRDenoError::Render("x".into());
    assert!(e.source().is_none());
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
    assert_eq!(cfg.isolate_pool_size, 1);
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

#[test]
fn config_default_source_maps() {
    let cfg = Config::default();
    assert!(!cfg.source_maps);
}

#[test]
fn config_is_clone() {
    let cfg = Config::default();
    let cfg2 = cfg.clone();
    assert_eq!(cfg.max_heap_size_mb, cfg2.max_heap_size_mb);
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
    let err = validate_render_timeout_ms(99).unwrap_err();
    assert!(err.contains("at least 100ms"), "got: {err}");
}

#[test]
fn validate_render_timeout_accepts_300000() {
    assert!(validate_render_timeout_ms(300_000).is_ok());
}

#[test]
fn validate_render_timeout_rejects_300001() {
    let err = validate_render_timeout_ms(300_001).unwrap_err();
    assert!(err.contains("300000ms"), "got: {err}");
}

// -----------------------------------------------------------------------
// validate_pool_size
// -----------------------------------------------------------------------

#[test]
fn validate_pool_size_rejects_zero() {
    let err = validate_pool_size(0).unwrap_err();
    assert!(matches!(err, SSRDenoError::WorkerInit(_)));
    assert!(format!("{err}").contains("at least 1"));
}

#[test]
fn validate_pool_size_accepts_one() {
    assert!(validate_pool_size(1).is_ok());
}

#[test]
fn validate_pool_size_accepts_large() {
    assert!(validate_pool_size(64).is_ok());
    assert!(validate_pool_size(256).is_ok());
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
        source_maps: false,
    }
}

#[test]
fn resolve_pool_size_uses_explicit_value() {
    assert_eq!(resolve_pool_size(make_cfg(4)), 4);
}

#[test]
fn resolve_pool_size_does_not_clamp_large() {
    assert_eq!(resolve_pool_size(make_cfg(99)), 99);
    assert_eq!(resolve_pool_size(make_cfg(1024)), 1024);
}

#[test]
fn resolve_pool_size_zero_clamps_to_one() {
    let size = resolve_pool_size(make_cfg(0));
    assert_eq!(size, 1);
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
    assert_eq!(next_index(&counter, 3), (usize::MAX - 1) % 3); // returns (MAX-1)%3, counter now MAX
    assert_eq!(next_index(&counter, 3), (usize::MAX) % 3); // returns MAX%3, counter wraps to 0
    assert_eq!(next_index(&counter, 3), 0); // returns 0 (old value), counter now 1
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
