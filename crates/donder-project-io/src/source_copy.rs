use crate::SourceProject;
use crate::address::DocumentId;
use std::collections::BTreeMap;

/// Rewrite document identities and imports using their resolved targets.
/// Local imports follow the renamed document paths.
pub(crate) fn remap_documents(
    source: &mut SourceProject,
    remaps: &BTreeMap<DocumentId, DocumentId>,
) -> Result<(), String> {
    let mut documents = indexmap::IndexMap::new();
    for (old_id, mut document) in std::mem::take(&mut source.documents) {
        let next_id = remaps
            .get(&old_id)
            .cloned()
            .unwrap_or_else(|| old_id.clone());
        for import in &mut document.imports {
            for target in &mut import.targets {
                if let Some(next) = remaps.get(target) {
                    *target = next.clone();
                }
            }
        }
        if documents.insert(next_id, document).is_some() {
            return Err("Copied document paths collide.".into());
        }
    }
    source.documents = documents;
    Ok(())
}
