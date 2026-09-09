use super::{GuiMutationError, ensure_owned_target};
use dawn_language::element::{ElementNodeKind, ElementTreeId};
use dawn_language::patch::{PatchId, PatchNode};
use dawn_language::preview::PreviewLayoutId;
use dawn_language::setup::{Setup, authoring::SetupLayoutCopy};
use dawn_project_io::{ProjectSession, SourceObjectKind};

pub(super) fn copy_layout(
    session: &mut ProjectSession,
    setup: &Setup,
) -> Result<(), GuiMutationError> {
    ensure_owned_target(session, &setup.id.0)?;
    let mut sequences = Vec::new();
    if setup.id == session.project.root.setup {
        for id in &session.project.root.sequences {
            let sequence =
                session.project.sequences.get(id).ok_or_else(|| {
                    GuiMutationError::Invalid("A show sequence is missing.".into())
                })?;
            let targets_tree = sequence
                .effects
                .iter()
                .any(|effect| effect.target.tree == setup.elements)
                || sequence
                    .control_clips
                    .iter()
                    .any(|clip| clip.target.selection().tree == setup.elements);
            if targets_tree {
                ensure_owned_target(session, &id.0)?;
                sequences.push(id.clone());
            }
        }
    }
    let (elements, preview) = super::create_layout_document(session, "layout_copy")?;
    let elements = ElementTreeId(elements);
    let preview = PreviewLayoutId(preview);
    let patch = PatchId(super::create_object_document(
        session,
        SourceObjectKind::Patch,
        "patch_copy",
        "patches",
        "patch",
    )?);
    let prop_ids = session.project.preview_layouts[&setup.preview]
        .props
        .iter()
        .map(|prop| prop.definition.clone())
        .collect::<indexmap::IndexSet<_>>();
    let profile_ids = session.project.element_trees[&setup.elements]
        .nodes
        .values()
        .filter_map(|node| {
            if let ElementNodeKind::Fixture { profile } = &node.kind {
                Some(profile.clone())
            } else {
                None
            }
        })
        .chain(
            session.project.patches[&setup.patch]
                .nodes
                .values()
                .filter_map(|node| node.fixture_profile().cloned()),
        )
        .collect::<indexmap::IndexSet<_>>();
    let mut props = indexmap::IndexMap::new();
    for original in prop_ids {
        let id = super::create_object_document(
            session,
            SourceObjectKind::PropDefinition,
            original.0.object(),
            "fixtures",
            "fixture",
        )
        .map(dawn_language::preview::PropDefinitionId)?;
        props.insert(original, id);
    }
    let mut profiles = indexmap::IndexMap::new();
    for original in profile_ids {
        let id = super::create_object_document(
            session,
            SourceObjectKind::FixtureProfile,
            original.0.object(),
            "fixture-profiles",
            "fixture-profile",
        )
        .map(dawn_language::fixture_profile::FixtureProfileId)?;
        profiles.insert(original, id);
    }
    dawn_language::setup::authoring::copy_setup_layout(
        &mut session.project,
        &setup.id,
        SetupLayoutCopy {
            elements: elements.clone(),
            preview: preview.clone(),
            patch: patch.clone(),
            props,
            profiles,
        },
        &sequences,
    )
    .map_err(GuiMutationError::Invalid)?;
    for node in session.project.patches[&patch].nodes.values_mut() {
        if let PatchNode::Source(source) = node {
            source.selection.tree = elements.clone();
        }
    }
    for (kind, identity) in [
        (SourceObjectKind::ElementTree, &elements.0),
        (SourceObjectKind::PreviewLayout, &preview.0),
        (SourceObjectKind::Patch, &patch.0),
    ] {
        dawn_project_io::ensure_document_can_reference_source(
            session,
            setup.id.0.document_id(),
            kind,
            identity,
        )
        .map_err(|error| GuiMutationError::Invalid(error.to_string()))?;
    }
    dawn_project_io::ensure_document_can_reference_source(
        session,
        preview.0.document_id(),
        SourceObjectKind::ElementTree,
        &elements.0,
    )
    .map_err(|error| GuiMutationError::Invalid(error.to_string()))?;
    for definition in session.project.preview_layouts[&preview]
        .props
        .iter()
        .map(|prop| prop.definition.0.clone())
        .collect::<Vec<_>>()
    {
        dawn_project_io::ensure_document_can_reference_source(
            session,
            preview.0.document_id(),
            SourceObjectKind::PropDefinition,
            &definition,
        )
        .map_err(|error| GuiMutationError::Invalid(error.to_string()))?;
    }
    for profile in session.project.element_trees[&elements]
        .nodes
        .values()
        .filter_map(|node| match &node.kind {
            ElementNodeKind::Fixture { profile } => Some(profile.0.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
    {
        dawn_project_io::ensure_document_can_reference_source(
            session,
            elements.0.document_id(),
            SourceObjectKind::FixtureProfile,
            &profile,
        )
        .map_err(|error| GuiMutationError::Invalid(error.to_string()))?;
    }
    ensure_patch_references(session, &patch)?;
    for id in sequences {
        dawn_project_io::ensure_document_can_reference_source(
            session,
            id.0.document_id(),
            SourceObjectKind::ElementTree,
            &elements.0,
        )
        .map_err(|error| GuiMutationError::Invalid(format!("{error:?}")))?;
    }
    Ok(())
}

pub(super) fn ensure_patch_references(
    session: &mut ProjectSession,
    patch: &PatchId,
) -> Result<(), GuiMutationError> {
    let references = session.project.patches[patch]
        .nodes
        .values()
        .flat_map(|node| {
            let mut references = Vec::new();
            match node {
                PatchNode::Source(source) => references.push((
                    SourceObjectKind::ElementTree,
                    source.selection.tree.0.clone(),
                )),
                PatchNode::Sink(sink) => {
                    references.push((SourceObjectKind::Controller, sink.controller.0.clone()))
                }
                PatchNode::Filter(_) => {}
            }
            if let Some(profile) = node.fixture_profile() {
                references.push((SourceObjectKind::FixtureProfile, profile.0.clone()));
            }
            references
        })
        .collect::<Vec<_>>();
    for (kind, identity) in references {
        dawn_project_io::ensure_document_can_reference_source(
            session,
            patch.0.document_id(),
            kind,
            &identity,
        )
        .map_err(|error| GuiMutationError::Invalid(format!("{error:?}")))?;
    }
    Ok(())
}
