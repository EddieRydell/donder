use std::collections::BTreeMap;

use camino::Utf8Path;
use dawn_package::{
    AssetDeclaration, AssetKind, ExportGroup, Lockfile, PackageManifest, ProjectManifest,
};
use uuid::Uuid;

pub fn write_project_package(root: &Utf8Path) {
    let mut assets = BTreeMap::new();
    collect_audio_assets(root, root, &mut assets);
    let manifest = PackageManifest {
        manifest_version: dawn_package::MANIFEST_VERSION,
        module_id: Uuid::new_v4(),
        language_version: dawn_package::LANGUAGE_VERSION.to_string(),
        project: Some(ProjectManifest {
            entrypoint: "project.dawn".to_string(),
        }),
        publication: None,
        exports: BTreeMap::from([(
            "project".to_string(),
            ExportGroup {
                documents: vec!["project.dawn".to_string()],
            },
        )]),
        dependencies: BTreeMap::new(),
        assets,
    };
    manifest.write(root).unwrap();
    Lockfile::new(&manifest, "https://registry.dawn.dev")
        .unwrap()
        .write(root)
        .unwrap();
}

fn collect_audio_assets(
    root: &Utf8Path,
    directory: &Utf8Path,
    assets: &mut BTreeMap<String, AssetDeclaration>,
) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let entry = entry.unwrap();
        let path = camino::Utf8PathBuf::from_path_buf(entry.path()).unwrap();
        if path.is_dir() {
            collect_audio_assets(root, &path, assets);
            continue;
        }
        let extension = path.extension().unwrap_or_default().to_ascii_lowercase();
        if matches!(extension.as_str(), "mp3" | "wav" | "ogg" | "flac") {
            assets.insert(
                path.strip_prefix(root).unwrap().as_str().replace('\\', "/"),
                AssetDeclaration {
                    kind: AssetKind::Audio,
                },
            );
        }
    }
}

pub fn load_project_package(root: &Utf8Path) -> dawn_project_io::ProjectSession {
    dawn_project_io::load_package(root).unwrap().session
}
