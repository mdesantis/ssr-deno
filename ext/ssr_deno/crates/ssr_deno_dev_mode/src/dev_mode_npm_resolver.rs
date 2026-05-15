use std::path::Path;

pub use deno_resolver::npm::{
    ByonmInNpmPackageChecker, ByonmNpmResolver, ByonmNpmResolverCreateOptions,
};
use node_resolver::cache::NodeResolutionSys;
use node_resolver::{PackageJsonResolver, PackageJsonResolverRc};

use ssr_deno_sys::Sys;

/// Builds a BYONM ("Bring Your Own node_modules") resolver trio for dev mode.
pub fn build_dev_mode_npm_resolver(
    project_root: &Path,
) -> (
    ByonmInNpmPackageChecker,
    ByonmNpmResolver<Sys>,
    PackageJsonResolverRc<Sys>,
) {
    let root_node_modules_dir = Some(project_root.join("node_modules"));
    let pkg_json_resolver: PackageJsonResolverRc<Sys> =
        PackageJsonResolverRc::new(PackageJsonResolver::new(Sys, None));
    let resolver = ByonmNpmResolver::new(ByonmNpmResolverCreateOptions {
        root_node_modules_dir,
        search_stop_dir: Some(project_root.to_path_buf()),
        sys: NodeResolutionSys::new(Sys, None),
        pkg_json_resolver: pkg_json_resolver.clone(),
    });

    (ByonmInNpmPackageChecker, resolver, pkg_json_resolver)
}
