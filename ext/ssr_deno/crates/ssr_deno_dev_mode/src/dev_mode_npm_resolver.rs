use std::borrow::Cow;
use std::path::Path;

pub use deno_resolver::npm::{
    ByonmInNpmPackageChecker, ByonmNpmResolver, ByonmNpmResolverCreateOptions,
};
use node_resolver::cache::NodeResolutionSys;
use node_resolver::{
    NodeConditionOptions, NodeResolverOptions, PackageJsonResolver, PackageJsonResolverRc,
};

use ssr_deno_sys::Sys;

/// `NodeResolverOptions` shared by every `NodeResolver` created in dev mode.
///
/// Splits ESM vs CJS condition sets to match Node's own defaults (`import`
/// for ESM, `require` for CJS). Without `require_conditions_override`,
/// `deno_node`'s `createRequire` resolves npm packages under
/// `["node","import"]`, which picks the `.cjs.mjs` ESM-wrapper for
/// emotion/MUI packages. Node then refuses `require()` of ESM in a cycle.
/// Splitting the overrides routes `require()` calls to the `.cjs.js` files
/// directly.
pub(crate) fn dev_node_resolver_options() -> NodeResolverOptions {
    NodeResolverOptions {
        conditions: NodeConditionOptions {
            conditions: vec![Cow::Borrowed("node")],
            import_conditions_override: Some(vec![Cow::Borrowed("node"), Cow::Borrowed("import")]),
            require_conditions_override: Some(vec![
                Cow::Borrowed("node"),
                Cow::Borrowed("require"),
            ]),
        },
        is_browser_platform: false,
        bundle_mode: true,
        typescript_version: None,
    }
}

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
