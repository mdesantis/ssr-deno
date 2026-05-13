use std::path::Path;

pub use deno_resolver::npm::{
    ByonmInNpmPackageChecker, ByonmNpmResolver, ByonmNpmResolverCreateOptions, ByonmNpmResolverRc,
};
use node_resolver::cache::NodeResolutionSys;
use node_resolver::{PackageJsonResolver, PackageJsonResolverRc};

use crate::sys::Sys;

/// Builds a BYONM ("Bring Your Own node_modules") resolver pair for dev mode.
///
/// The resolver walks the user's `node_modules/` directory directly — no
/// lockfile, no `.deno` directory. Supports plain npm, pnpm (symlinked),
/// and Yarn layouts. The caller wires these into `NodeExtInitServices` and
/// `WorkerServiceOptions` (see [`dev_builder`](super::deno_runtime_wrapper::dev_builder)).
///
/// **Precondition:** `project_root/node_modules/` should exist before the
/// first render. If missing, bare-specifier resolution fails at render time
/// (not at worker construction). User-facing error surfaces as a module-load
/// failure with the unresolved specifier.
///
/// `search_stop_dir = project_root` caps Byonm's ancestor walk for
/// `package.json` lookup; redundant with the read-permission boundary
/// (Permissions already deny reads above `project_root`) but cheap
/// defense-in-depth and avoids unnecessary syscalls.
// Step 6 wires this — until then, dev-mode builds emit a dead-code warning
// without the allow attribute.
#[allow(dead_code)]
pub fn build_dev_npm_resolver(
    project_root: &Path,
) -> (ByonmInNpmPackageChecker, ByonmNpmResolverRc<Sys>) {
    let root_node_modules_dir = Some(project_root.join("node_modules"));
    let pkg_json_resolver: PackageJsonResolverRc<Sys> =
        PackageJsonResolverRc::new(PackageJsonResolver::new(Sys, None));
    let resolver = ByonmNpmResolver::new(ByonmNpmResolverCreateOptions {
        root_node_modules_dir,
        search_stop_dir: Some(project_root.to_path_buf()),
        sys: NodeResolutionSys::new(Sys, None),
        pkg_json_resolver,
    });

    (ByonmInNpmPackageChecker, ByonmNpmResolverRc::new(resolver))
}
