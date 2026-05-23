use super::*;

#[test]
fn dev_node_resolver_options_fields() {
    let opts = dev_node_resolver_options();
    assert!(!opts.is_browser_platform);
    assert!(opts.bundle_mode);
}
