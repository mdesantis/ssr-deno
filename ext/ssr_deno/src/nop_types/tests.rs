use std::borrow::Cow;

use node_resolver::InNpmPackageChecker;
use node_resolver::NpmPackageFolderResolver;

use super::*;

#[test]
fn in_npm_package_always_false() {
    let checker = NopInNpmPackageChecker;
    let url = Url::parse("https://registry.npmjs.org/some-pkg").unwrap();
    assert!(!checker.in_npm_package(&url));
}

#[test]
fn resolve_package_folder_from_package_always_not_found() {
    let resolver = NopNpmPackageFolderResolver;
    let referrer_url = Url::parse("file:///dev/null").unwrap();
    let referrer = node_resolver::UrlOrPathRef::from_url(&referrer_url);
    let result = resolver.resolve_package_folder_from_package("some-pkg", &referrer);
    assert!(result.is_err());
}

#[test]
fn resolve_types_package_folder_always_none() {
    let resolver = NopNpmPackageFolderResolver;
    assert!(
        resolver
            .resolve_types_package_folder("some-pkg", None, None)
            .is_none()
    );
}

#[test]
fn parse_read_and_write_descriptors_ok() {
    let parser = NopPermissionDescriptorParser;
    assert!(parser.parse_read_descriptor("/some/path").is_ok());
    assert!(parser.parse_write_descriptor("/some/path").is_ok());
}

#[test]
fn parse_net_and_import_descriptors_ok() {
    let parser = NopPermissionDescriptorParser;
    assert!(parser.parse_net_descriptor("example.com:443").is_ok());
    assert!(parser.parse_import_descriptor("example.com:443").is_ok());
    assert!(parser.parse_net_query("example.com").is_ok());
}

#[test]
fn parse_env_descriptor_ok() {
    let parser = NopPermissionDescriptorParser;
    assert!(parser.parse_env_descriptor("PATH").is_ok());
}

#[test]
fn parse_sys_descriptor_ok() {
    let parser = NopPermissionDescriptorParser;
    assert!(parser.parse_sys_descriptor("hostname").is_ok());
}

#[test]
fn parse_allow_and_deny_run_descriptors_ok() {
    let parser = NopPermissionDescriptorParser;
    // "ls" may or may not resolve on PATH depending on the environment;
    // either way AllowRunDescriptor::parse itself succeeds (an unresolved
    // binary is a Descriptor variant, not an Err — see
    // AllowRunDescriptorParseResult::Unresolved).
    assert!(parser.parse_allow_run_descriptor("ls").is_ok());
    assert!(parser.parse_deny_run_descriptor("ls").is_ok());
}

#[test]
fn parse_ffi_descriptor_ok() {
    let parser = NopPermissionDescriptorParser;
    assert!(parser.parse_ffi_descriptor("/some/path").is_ok());
}

#[test]
fn parse_path_query_and_special_file_descriptor_ok() {
    let parser = NopPermissionDescriptorParser;
    let cwd = std::env::current_dir().unwrap();
    let path_query = parser
        .parse_path_query(Cow::Owned(cwd))
        .expect("cwd is a valid, non-empty path");
    assert!(parser.parse_special_file_descriptor(path_query).is_ok());
}

#[test]
fn parse_run_query_ok() {
    let parser = NopPermissionDescriptorParser;
    // Falls back to the `Name` variant if "ls" isn't found on PATH — see
    // RunQueryDescriptor::parse, which never errors on a which() miss.
    assert!(parser.parse_run_query("ls").is_ok());
}
