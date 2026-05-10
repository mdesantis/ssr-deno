//! NOP (no-operation) implementations for the generic type parameters required
//! by `MainWorker::bootstrap_from_options`.
//!
//! These types are never actually called because our SSR bundle is self-
//! contained and does not import npm packages or use Node.js APIs.

use std::path::PathBuf;

use deno_runtime::deno_core::url::Url;
use deno_runtime::deno_permissions::AllowRunDescriptor;
use deno_runtime::deno_permissions::AllowRunDescriptorParseResult;
use deno_runtime::deno_permissions::DenyRunDescriptor;
use deno_runtime::deno_permissions::EnvDescriptor;
use deno_runtime::deno_permissions::EnvDescriptorParseError;
use deno_runtime::deno_permissions::FfiDescriptor;
use deno_runtime::deno_permissions::ImportDescriptor;
use deno_runtime::deno_permissions::NetDescriptor;
use deno_runtime::deno_permissions::NetDescriptorParseError;
use deno_runtime::deno_permissions::PathDescriptor;
use deno_runtime::deno_permissions::PathQueryDescriptor;
use deno_runtime::deno_permissions::PathResolveError;
use deno_runtime::deno_permissions::PermissionDescriptorParser;
use deno_runtime::deno_permissions::ReadDescriptor;
use deno_runtime::deno_permissions::RunDescriptorParseError;
use deno_runtime::deno_permissions::RunQueryDescriptor;
use deno_runtime::deno_permissions::SpecialFilePathQueryDescriptor;
use deno_runtime::deno_permissions::SysDescriptor;
use deno_runtime::deno_permissions::SysDescriptorParseError;
use deno_runtime::deno_permissions::WriteDescriptor;

use crate::sys::Sys;

// ---------------------------------------------------------------------------
// NopInNpmPackageChecker
// ---------------------------------------------------------------------------

/// NOP implementation of [`node_resolver::InNpmPackageChecker`].
///
/// Always returns `false` — no specifier is ever considered to be inside an
/// npm package.
#[derive(Debug, Clone)]
pub struct NopInNpmPackageChecker;

impl node_resolver::InNpmPackageChecker for NopInNpmPackageChecker {
    fn in_npm_package(&self, _specifier: &Url) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// NopNpmPackageFolderResolver
// ---------------------------------------------------------------------------

/// NOP implementation of [`node_resolver::NpmPackageFolderResolver`].
///
/// Always returns an error — npm package resolution is not supported.
#[derive(Debug, Clone)]
pub struct NopNpmPackageFolderResolver;

impl node_resolver::NpmPackageFolderResolver for NopNpmPackageFolderResolver {
    fn resolve_package_folder_from_package(
        &self,
        specifier: &str,
        _referrer: &node_resolver::UrlOrPathRef,
    ) -> Result<PathBuf, node_resolver::errors::PackageFolderResolveError> {
        Err(
            node_resolver::errors::PackageFolderResolveErrorKind::PackageNotFound(
                node_resolver::errors::PackageNotFoundError {
                    package_name: specifier.to_string(),
                    // Unix-only extension (libc::isatty, etc.). This URL is a
                    // stub for an error type — never actually used as a path.
                    referrer: node_resolver::UrlOrPath::Url(
                        deno_runtime::deno_core::url::Url::parse("file:///dev/null")
                            .expect("Valid URL"),
                    ),
                    referrer_extra: None,
                },
            )
            .into(),
        )
    }

    fn resolve_types_package_folder(
        &self,
        _types_package_name: &str,
        _maybe_package_version: Option<&deno_semver::Version>,
        _maybe_referrer: Option<&node_resolver::UrlOrPathRef>,
    ) -> Option<PathBuf> {
        None
    }
}

// ---------------------------------------------------------------------------
// NopPermissionDescriptorParser
// ---------------------------------------------------------------------------

/// Minimal [`PermissionDescriptorParser`] required by the `PermissionsContainer`
/// constructor. The SSR worker runs with `Permissions::none_without_prompt()` so
/// this parser is never invoked at runtime — it only satisfies the type signature.
#[derive(Debug)]
pub struct NopPermissionDescriptorParser;

impl PermissionDescriptorParser for NopPermissionDescriptorParser {
    fn parse_read_descriptor(&self, text: &str) -> Result<ReadDescriptor, PathResolveError> {
        Ok(ReadDescriptor(PathDescriptor::new_known_absolute(
            std::borrow::Cow::Owned(PathBuf::from(text)),
        )))
    }

    fn parse_write_descriptor(&self, text: &str) -> Result<WriteDescriptor, PathResolveError> {
        Ok(WriteDescriptor(PathDescriptor::new_known_absolute(
            std::borrow::Cow::Owned(PathBuf::from(text)),
        )))
    }

    fn parse_net_descriptor(&self, text: &str) -> Result<NetDescriptor, NetDescriptorParseError> {
        NetDescriptor::parse_for_list(text)
    }

    fn parse_import_descriptor(
        &self,
        text: &str,
    ) -> Result<ImportDescriptor, NetDescriptorParseError> {
        ImportDescriptor::parse_for_list(text)
    }

    fn parse_env_descriptor(&self, text: &str) -> Result<EnvDescriptor, EnvDescriptorParseError> {
        Ok(EnvDescriptor::new(std::borrow::Cow::Owned(
            text.to_string(),
        )))
    }

    fn parse_sys_descriptor(&self, text: &str) -> Result<SysDescriptor, SysDescriptorParseError> {
        SysDescriptor::parse(text.to_string())
    }

    fn parse_allow_run_descriptor(
        &self,
        text: &str,
    ) -> Result<AllowRunDescriptorParseResult, RunDescriptorParseError> {
        let cwd = std::env::current_dir().map_err(PathResolveError::CwdResolve)?;
        AllowRunDescriptor::parse(text, &cwd, &Sys).map_err(RunDescriptorParseError::Which)
    }

    fn parse_deny_run_descriptor(&self, text: &str) -> Result<DenyRunDescriptor, PathResolveError> {
        let cwd = std::env::current_dir().map_err(PathResolveError::CwdResolve)?;
        Ok(DenyRunDescriptor::parse(text, &cwd))
    }

    fn parse_ffi_descriptor(&self, text: &str) -> Result<FfiDescriptor, PathResolveError> {
        Ok(FfiDescriptor(PathDescriptor::new_known_absolute(
            std::borrow::Cow::Owned(PathBuf::from(text)),
        )))
    }

    fn parse_path_query<'a>(
        &self,
        path: std::borrow::Cow<'a, std::path::Path>,
    ) -> Result<PathQueryDescriptor<'a>, PathResolveError> {
        PathQueryDescriptor::new(&Sys, path)
    }

    fn parse_special_file_descriptor<'a>(
        &self,
        path: PathQueryDescriptor<'a>,
    ) -> Result<SpecialFilePathQueryDescriptor<'a>, PathResolveError> {
        SpecialFilePathQueryDescriptor::parse(&Sys, path)
    }

    fn parse_net_query(&self, text: &str) -> Result<NetDescriptor, NetDescriptorParseError> {
        NetDescriptor::parse_for_query(text)
    }

    fn parse_run_query<'a>(
        &self,
        requested: &'a str,
    ) -> Result<RunQueryDescriptor<'a>, RunDescriptorParseError> {
        RunQueryDescriptor::parse(requested, &Sys).map_err(RunDescriptorParseError::PathResolve)
    }
}
