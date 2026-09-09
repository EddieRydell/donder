use crate::dto::{SetupControlElement, SetupIndexedOption};
use crate::gui::{GuiMutationError, model::source_identity_from_gui};
use dawn_language::element::ElementTreeId;
use dawn_language::element::{
    ElementNode, ElementNodeId, ElementNodeKind, IndexedOption, IndexedOptionId,
};
use dawn_language::fixture_profile::FixtureProfileId;
use dawn_project_io::{ProjectSession, SourceObjectKind};

pub(super) fn project_kind(kind: &ElementNodeKind) -> Option<SetupControlElement> {
    match kind {
        ElementNodeKind::Scalar { cells } => Some(SetupControlElement::Scalar { cells: *cells }),
        ElementNodeKind::Indexed { cells, options } => Some(SetupControlElement::Indexed {
            cells: *cells,
            options: options
                .iter()
                .map(|option| SetupIndexedOption {
                    id: option.id.0,
                    name: option.name.clone(),
                })
                .collect(),
        }),
        ElementNodeKind::Fixture { profile } => Some(SetupControlElement::Fixture {
            profile: crate::gui::patch::object_ref(&profile.0, SourceObjectKind::FixtureProfile),
        }),
        ElementNodeKind::Group { .. } | ElementNodeKind::Color { .. } => None,
    }
}

pub(super) fn edit(
    session: &mut ProjectSession,
    tree_id: &ElementTreeId,
    id: Option<u32>,
    name: String,
    parent: Option<u32>,
    definition: SetupControlElement,
) -> Result<(), GuiMutationError> {
    super::ensure_owned_target(session, &tree_id.0)?;
    if name.trim().is_empty() {
        return Err(GuiMutationError::Invalid("Give the element a name.".into()));
    }
    let kind = match definition {
        SetupControlElement::Scalar { cells } => ElementNodeKind::Scalar { cells },
        SetupControlElement::Indexed { cells, options } => {
            if options.iter().any(|option| option.name.trim().is_empty()) {
                return Err(GuiMutationError::Invalid(
                    "Give every indexed option a name.".into(),
                ));
            }
            ElementNodeKind::Indexed {
                cells,
                options: options
                    .into_iter()
                    .map(|option| IndexedOption {
                        id: IndexedOptionId(option.id),
                        name: option.name,
                    })
                    .collect(),
            }
        }
        SetupControlElement::Fixture { profile } => {
            let identity =
                source_identity_from_gui(&profile.module_id, &profile.path, &profile.object_key)?;
            dawn_project_io::ensure_document_can_reference_source(
                session,
                tree_id.0.document_id(),
                SourceObjectKind::FixtureProfile,
                &identity,
            )
            .map_err(|error| GuiMutationError::Invalid(format!("{error:?}")))?;
            ElementNodeKind::Fixture {
                profile: FixtureProfileId(identity),
            }
        }
    };
    match id {
        Some(id) => {
            let original = &session.project.element_trees[tree_id]
                .nodes
                .get(&ElementNodeId(id))
                .ok_or_else(|| GuiMutationError::Invalid("Element was not found.".into()))?
                .kind;
            let resize = match (original, &kind) {
                (ElementNodeKind::Scalar { cells: old }, ElementNodeKind::Scalar { cells })
                | (
                    ElementNodeKind::Indexed { cells: old, .. },
                    ElementNodeKind::Indexed { cells, .. },
                ) if old != cells => Some(*cells),
                _ => None,
            };
            if let Some(cells) = resize {
                let changed = dawn_language::setup::authoring::resize_control_outputs(
                    &mut session.project,
                    tree_id,
                    ElementNodeId(id),
                    cells,
                )
                .map_err(GuiMutationError::Invalid)?;
                for patch in changed {
                    super::ensure_owned_target(session, &patch.0)?;
                }
            }
            let node = super::tree_mut(session, tree_id)?
                .nodes
                .get_mut(&ElementNodeId(id))
                .ok_or_else(|| GuiMutationError::Invalid("Element was not found.".into()))?;
            if !matches!(
                node.kind,
                ElementNodeKind::Scalar { .. }
                    | ElementNodeKind::Indexed { .. }
                    | ElementNodeKind::Fixture { .. }
            ) {
                return Err(GuiMutationError::Invalid(
                    "Choose a scalar, indexed, or fixture element.".into(),
                ));
            }
            *node = ElementNode { name, kind };
        }
        None => {
            dawn_language::element::authoring::add_element(
                &mut session.project,
                tree_id,
                ElementNode { name, kind },
                parent.map(ElementNodeId),
            )
            .map_err(GuiMutationError::Invalid)?;
        }
    }
    Ok(())
}
