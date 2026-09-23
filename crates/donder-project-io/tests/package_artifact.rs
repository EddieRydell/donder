use std::collections::BTreeMap;
use std::fs;

use camino::Utf8Path;
use donder_package::{
    CacheStore, ExportGroup, LockedPackage, Lockfile, PackageId, PackageManifest, Publication,
};
use semver::Version;
use tempfile::tempdir;
use uuid::Uuid;

#[test]
fn compiler_rejection_prevents_registry_artifact_acceptance() {
    let directory = tempdir().expect("tempdir");
    let root = Utf8Path::from_path(directory.path()).expect("utf8");
    fs::write(root.join("broken.operator.donder"), "{").expect("invalid Donder source");

    let package = PackageId::new("alice/broken").expect("package identity");
    let version = Version::parse("1.0.0").expect("version");
    let module_id = Uuid::new_v4();
    let manifest = PackageManifest {
        manifest_version: donder_package::MANIFEST_VERSION,
        module_id,
        language_version: donder_package::LANGUAGE_VERSION.to_string(),
        project: None,
        publication: Some(Publication {
            package: package.clone(),
            version: version.clone(),
            display_name: "Broken".to_string(),
            summary: "Compiler rejection fixture".to_string(),
            license: "MIT".to_string(),
            tags: Vec::new(),
        }),
        exports: BTreeMap::from([(
            "operators".to_string(),
            ExportGroup {
                documents: vec!["broken.operator.donder".to_string()],
            },
        )]),
        dependencies: BTreeMap::new(),
        assets: BTreeMap::new(),
    };
    manifest.write(root).expect("manifest");

    let locked = LockedPackage {
        version,
        archive_sha256: "a".repeat(64),
        module_id,
        dependencies: BTreeMap::new(),
    };
    let lock = Lockfile {
        lock_version: donder_package::LOCK_VERSION,
        manifest_sha256: "b".repeat(64),
        registry: "https://registry.donder.dev".to_string(),
        packages: BTreeMap::from([(package.clone(), locked.clone())]),
        path_dependencies: BTreeMap::new(),
    };
    let cache = CacheStore::new(root.join("cache"));

    let error = donder_project_io::validate_registry_package_artifact(
        root, &package, &locked, &lock, &cache,
    )
    .expect_err("invalid exported source must fail compiler validation");

    assert!(error.to_string().contains("failed compiler validation"));
}
