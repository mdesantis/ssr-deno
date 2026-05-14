use std::path::Path;
use std::sync::Arc;

use deno_resolver::cache::{ParsedSourceCache, ParsedSourceCacheRc};
use deno_resolver::cjs::analyzer::{
    DenoAstModuleExportAnalyzer, DenoCjsCodeAnalyzer, NodeAnalysisCacheRc, NullNodeAnalysisCache,
};
use deno_resolver::cjs::{CjsTracker, IsCjsResolutionMode};
use deno_resolver::loader::NpmModuleLoader;
use deno_resolver::npm::DenoInNpmPackageChecker;
pub use deno_resolver::npm::{
    ByonmInNpmPackageChecker, ByonmNpmResolver, ByonmNpmResolverCreateOptions,
};
use node_resolver::analyze::{CjsModuleExportAnalyzer, NodeCodeTranslator, NodeCodeTranslatorMode};
use node_resolver::cache::NodeResolutionSys;
use node_resolver::{
    DenoIsBuiltInNodeModuleChecker, NodeConditionOptions, NodeResolver, NodeResolverOptions,
    PackageJsonResolver, PackageJsonResolverRc,
};

use crate::sys::Sys;

/// Builds a BYONM ("Bring Your Own node_modules") resolver trio for dev mode.
pub fn build_dev_npm_resolver(
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

// Type alias for the full NpmModuleLoader chain used in dev mode.
// TCjsCodeAnalyzer = DenoCjsCodeAnalyzer<Sys>
// TInNpmPackageChecker = DenoInNpmPackageChecker (wraps ByonmInNpmPackageChecker)
// TIsBuiltInNodeModuleChecker = DenoIsBuiltInNodeModuleChecker
// TNpmPackageFolderResolver = ByonmNpmResolver<Sys>
// TSys = Sys
pub type DevNpmModuleLoader = NpmModuleLoader<
    DenoCjsCodeAnalyzer<Sys>,
    DenoInNpmPackageChecker,
    DenoIsBuiltInNodeModuleChecker,
    ByonmNpmResolver<Sys>,
    Sys,
>;

/// Build the full CJS→ESM translation stack for dev mode.
///
/// Returns the NpmModuleLoader.
pub fn build_dev_npm_module_loader(
    _project_root: &Path,
    npm_resolver: ByonmNpmResolver<Sys>,
    pkg_json_resolver: PackageJsonResolverRc<Sys>,
) -> Arc<DevNpmModuleLoader> {
    let checker = DenoInNpmPackageChecker::Byonm(ByonmInNpmPackageChecker);

    // Node resolver (needed for CJS re-export analysis)
    let node_resolver: Arc<
        NodeResolver<
            DenoInNpmPackageChecker,
            DenoIsBuiltInNodeModuleChecker,
            ByonmNpmResolver<Sys>,
            Sys,
        >,
    > = Arc::new(NodeResolver::new(
        checker.clone(),
        DenoIsBuiltInNodeModuleChecker,
        npm_resolver.clone(),
        pkg_json_resolver.clone(),
        NodeResolutionSys::new(Sys, None),
        NodeResolverOptions {
            conditions: NodeConditionOptions {
                conditions: vec![
                    std::borrow::Cow::Borrowed("node"),
                    std::borrow::Cow::Borrowed("import"),
                ],
                import_conditions_override: None,
                require_conditions_override: None,
            },
            is_browser_platform: false,
            bundle_mode: true,
            typescript_version: None,
        },
    ));

    // CjsTracker — detects whether a file is CJS
    let cjs_tracker: Arc<CjsTracker<_, _>> = Arc::new(CjsTracker::new(
        checker.clone(),
        pkg_json_resolver.clone(),
        IsCjsResolutionMode::ImplicitTypeCommonJs,
        vec![],
    ));

    // CJS code analyzer.
    //
    // `NullNodeAnalysisCache` skips persistence of analysis results across
    // calls; revisit if reload-time profiling shows analyze churn. The
    // `ParsedSourceCache` is the deno_ast parsed-program cache — required
    // by `DenoAstModuleExportAnalyzer` for re-export resolution.
    //
    // The placeholder `NotImplementedModuleExportAnalyzer` panics on call;
    // gating the `deno_resolver/deno_ast` feature unlocks the real
    // `DenoAstModuleExportAnalyzer`. Without it, the first CJS file load
    // (eg `@emotion/cache/dist/emotion-cache.cjs.js`) crashes the worker.
    let cache: NodeAnalysisCacheRc = Arc::new(NullNodeAnalysisCache);
    let parsed_source_cache: ParsedSourceCacheRc =
        ParsedSourceCacheRc::new(ParsedSourceCache::default());
    let code_analyzer = DenoCjsCodeAnalyzer::new(
        cache,
        cjs_tracker.clone(),
        Arc::new(DenoAstModuleExportAnalyzer::new(parsed_source_cache)),
        Sys,
    );

    // Module export analyzer wraps the code analyzer and resolves re-exports
    let module_export_analyzer = Arc::new(CjsModuleExportAnalyzer::new(
        code_analyzer,
        checker.clone(),
        node_resolver,
        npm_resolver,
        pkg_json_resolver,
        Sys,
    ));

    // NodeCodeTranslator — translates CJS source to ESM-compatible source
    let node_code_translator = Arc::new(NodeCodeTranslator::new(
        module_export_analyzer,
        NodeCodeTranslatorMode::ModuleLoader,
    ));

    // Assemble the full NpmModuleLoader
    Arc::new(NpmModuleLoader::new(cjs_tracker, node_code_translator, Sys))
}
