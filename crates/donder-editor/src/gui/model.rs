pub(super) fn register_sequence_audio_asset(
    session: &mut ProjectSession,
    document: &donder_model::DocumentId,
    import_path: &str,
) -> Result<AssetId, GuiMutationError> {
    if let Some(asset) = session.source.referenced_assets.iter_mut().find(|asset| {
        asset.module_id == document.module_id() && asset.relative_path.as_str() == import_path
    }) {
        asset.referenced_by.insert(document.clone());
        return Ok(asset.id.clone());
    }

    if !session.source.is_project_owned(document) {
        return Err(GuiMutationError::Invalid(
            "Audio must belong to this project.".into(),
        ));
    }
    donder_project_io::validate_relative_path(import_path).map_err(GuiMutationError::Invalid)?;
    let selected_path = session.source.project_root().join(import_path);
    let absolute_path = fs::canonicalize(&selected_path)
        .map_err(|error| GuiMutationError::Invalid(format!("Audio file was not found: {error}")))?;
    let absolute_path = Utf8PathBuf::from_path_buf(absolute_path).map_err(|path| {
        GuiMutationError::Invalid(format!("Audio path is not valid UTF-8: {}", path.display()))
    })?;
    if !absolute_path.is_file() || !absolute_path.starts_with(session.source.project_root()) {
        return Err(GuiMutationError::Invalid(
            "Selected audio path is not a file inside the project module.".to_string(),
        ));
    }

    if let Some(asset) = session.source.referenced_assets.iter().find(|asset| {
        asset.module_id == document.module_id() && asset.absolute_path == absolute_path
    }) {
        return Ok(asset.id.clone());
    }

    let relative_path = Utf8PathBuf::from(import_path);
    let next_id = session
        .source
        .referenced_assets
        .iter()
        .map(|asset| asset.id.0)
        .max()
        .unwrap_or(0)
        .saturating_add(1);
    let id = AssetId(next_id);
    session.source.referenced_assets.push(ReferencedAsset {
        id: id.clone(),
        module_id: document.module_id(),
        relative_path,
        absolute_path,
        referenced_by: std::collections::BTreeSet::from([document.clone()]),
    });
    Ok(id)
}

pub(super) fn effect_mut(
    sequence: &mut donder_model::Sequence,
    id: u32,
) -> Result<&mut EffectInst, GuiMutationError> {
    sequence
        .effects
        .iter_mut()
        .find(|effect| effect.id.0 == id)
        .ok_or_else(|| GuiMutationError::Invalid("Effect was not found.".to_string()))
}

pub(super) fn composition_graph_node_mut<'a>(
    sequence: &'a mut donder_model::Sequence,
    id: &CompositionGraphNodeId,
) -> Result<&'a mut CompositionGraphNode, GuiMutationError> {
    sequence
        .composition_graph
        .nodes
        .iter_mut()
        .find(|node| node.id == *id)
        .ok_or_else(|| GuiMutationError::Invalid("Graph node was not found.".to_string()))
}

pub(super) fn parse_graph_node_id(value: &str) -> Result<CompositionGraphNodeId, GuiMutationError> {
    if let Some(id) = value.strip_prefix("node:") {
        return id
            .parse::<u32>()
            .map(CompositionGraphNodeId)
            .map_err(|_| GuiMutationError::Invalid("Invalid graph node id.".to_string()));
    }
    Err(GuiMutationError::Invalid(
        "Invalid graph node id.".to_string(),
    ))
}

pub(super) fn ensure_graph_node_exists(
    sequence: &donder_model::Sequence,
    node_id: &CompositionGraphNodeId,
) -> Result<(), GuiMutationError> {
    if sequence
        .composition_graph
        .nodes
        .iter()
        .any(|node| node.id == *node_id)
    {
        Ok(())
    } else {
        Err(GuiMutationError::Invalid(
            "Graph node was not found.".to_string(),
        ))
    }
}

pub(super) fn graph_input_cardinality(
    definitions: &donder_model::OperatorDefinitionStore,
    kind: &CompositionGraphNodeKind,
    source_name: &str,
) -> Option<OperatorPortCardinality> {
    match kind {
        CompositionGraphNodeKind::Layer { .. } => None,
        CompositionGraphNodeKind::Operator(operator) => definitions
            .resolve(&operator.operator)?
            .inputs()
            .iter()
            .find(|port| port.source_name == source_name)
            .map(|port| port.cardinality.clone()),
        CompositionGraphNodeKind::Output => {
            (source_name == "input").then_some(OperatorPortCardinality::Many)
        }
    }
}

pub(super) fn next_composition_node_id(sequence: &donder_model::Sequence) -> u32 {
    sequence
        .composition_graph
        .nodes
        .iter()
        .map(|node| node.id.0)
        .max()
        .unwrap_or(0)
        + 1
}

pub(super) fn create_sequence_layer(
    sequence: &mut donder_model::Sequence,
    name: String,
    color: String,
    position: Option<(f32, f32)>,
    connect_to_output: bool,
) -> Result<(), GuiMutationError> {
    let next_layer_id = sequence
        .layers
        .iter()
        .map(|layer| layer.id.0)
        .max()
        .unwrap_or(0)
        + 1;
    let output_node_id = sequence
        .composition_graph
        .nodes
        .iter()
        .find(|node| matches!(node.kind, CompositionGraphNodeKind::Output))
        .map(|node| node.id.clone())
        .ok_or_else(|| {
            GuiMutationError::Invalid("Composition graph output was not found.".to_string())
        })?;
    let layer_node_id = CompositionGraphNodeId(next_composition_node_id(sequence));
    typed_name(&name)?;
    let name = fresh_name(&name, |candidate| sequence_name_taken(sequence, candidate));
    sequence.layers.push(donder_model::SequenceLayer {
        id: SequenceLayerId(next_layer_id),
        name,
        description: None,
        color: parse_color(&color)?,
        enabled: true,
    });
    let (x, y) = position.unwrap_or((80.0, 120.0 + next_layer_id as f32 * 80.0));
    sequence.composition_graph.nodes.push(CompositionGraphNode {
        id: layer_node_id.clone(),
        position: GraphNodePosition { x, y },
        kind: CompositionGraphNodeKind::Layer {
            layer_id: SequenceLayerId(next_layer_id),
        },
    });
    if connect_to_output {
        sequence.composition_graph.edges.push(EffectGraphEdge {
            from: layer_node_id,
            from_port: GraphPortId("output".to_string()),
            to: output_node_id,
            to_port: GraphPortId("input".to_string()),
        });
    }
    Ok(())
}

pub(super) fn graph_operator_from_gui(
    session: &ProjectSession,
    operator: &SequenceGraphOperator,
) -> Result<OperatorRef, GuiMutationError> {
    Ok(match operator {
        SequenceGraphOperator::Custom {
            module_id,
            path,
            object_key,
        } => {
            let identity =
                source_identity_from_gui(module_id, path, identifier(object_key)?.as_str())?;
            if !session.source.is_project_owned(identity.document_id()) {
                return Err(GuiMutationError::Invalid(
                    "Operator source module was not found.".to_string(),
                ));
            }
            OperatorRef::Custom(OperatorDefinitionId(identity))
        }
    })
}

pub fn source_identity_from_gui(
    module_id: &str,
    path: &str,
    object: &str,
) -> Result<SourceIdentity, GuiMutationError> {
    let module_id = uuid::Uuid::parse_str(module_id)
        .map_err(|_| GuiMutationError::Invalid("Source module ID is invalid.".to_string()))?;
    Ok(SourceIdentity::from_document(
        donder_model::DocumentId::new(module_id, Utf8PathBuf::from(path)),
        object.to_string(),
    ))
}

/// Whether a name is used by a layer, graph node or the output node: they
/// share the graph's namespace.
pub(super) fn sequence_name_taken(sequence: &donder_model::Sequence, name: &str) -> bool {
    name == "output"
        || sequence.layers.iter().any(|layer| layer.name.as_str() == name)
        || sequence
            .composition_graph
            .nodes
            .iter()
            .any(|node| matches!(&node.kind, CompositionGraphNodeKind::Operator(operator) if operator.name.as_str() == name))
}

pub(super) fn mark_collection_mut<'a>(
    sequence: &'a mut donder_model::Sequence,
    key: &str,
) -> Result<&'a mut MarkCollection, GuiMutationError> {
    sequence
        .mark_collections
        .iter_mut()
        .find(|collection| collection.key.name.as_str() == key)
        .ok_or_else(|| GuiMutationError::Invalid("Mark collection was not found.".to_string()))
}

pub(super) fn automation_clip_mut(
    sequence: &mut donder_model::Sequence,
    id: u32,
) -> Result<&mut AutomationClip, GuiMutationError> {
    sequence
        .automation_clips
        .iter_mut()
        .find(|clip| clip.id.0 == id)
        .ok_or_else(|| GuiMutationError::Invalid("Automation clip was not found.".to_string()))
}

/// A name typed in the GUI, as an object name: `Porch Left` becomes
/// `porch_left`. Uniqueness is checked by the edited object's validation.
pub(super) fn typed_name(text: &str) -> Result<Identifier, GuiMutationError> {
    if !text
        .chars()
        .any(|character| character.is_ascii_alphanumeric())
    {
        return Err(GuiMutationError::Invalid(
            "Names need at least one letter or digit.".into(),
        ));
    }
    Ok(donder_language::object_name(text))
}

/// A fresh name from `text`, made unique against `taken`.
pub(super) fn fresh_name(text: &str, taken: impl Fn(&str) -> bool) -> Identifier {
    donder_language::unique_name(donder_language::object_name(text).as_str(), taken)
}

pub(super) fn identifier(value: &str) -> Result<Identifier, GuiMutationError> {
    Identifier::new(value.to_string())
        .map_err(|_| GuiMutationError::Invalid(format!("Invalid identifier `{value}`.")))
}

pub(super) fn effect_scope(scope: SequenceEffectScope) -> EffectScope {
    match scope {
        SequenceEffectScope::PerFixture => EffectScope::PerFixture,
        SequenceEffectScope::WholeTarget => EffectScope::WholeTarget,
    }
}

pub(super) fn layout_target_to_effect_target(
    layout: &LayoutId,
    target: FixtureTarget,
) -> DomainFixtureTarget {
    DomainFixtureTarget {
        layout: layout.clone(),
        fixture: FixtureInstanceId(target.fixture),
    }
}

pub fn effect_param_value_from_gui(
    session: &mut ProjectSession,
    owner: &SourceIdentity,
    value: SequenceEffectParamValue,
) -> Result<EffectParamValue, GuiMutationError> {
    Ok(match value {
        SequenceEffectParamValue::Int { value } => EffectParamValue::Int(value as i32),
        SequenceEffectParamValue::Float { value } => EffectParamValue::Float(value),
        SequenceEffectParamValue::Bool { value } => EffectParamValue::Bool(value),
        SequenceEffectParamValue::Color { value } => EffectParamValue::Color(parse_color(&value)?),
        SequenceEffectParamValue::Enum { value } => EffectParamValue::Enum(identifier(&value)?),
        SequenceEffectParamValue::Marks { key } => EffectParamValue::Marks(MarkCollectionKey {
            name: identifier(&key)?,
        }),
        SequenceEffectParamValue::Curve { value } => EffectParamValue::Curve(
            match library_identity(session, owner, SourceObjectKind::Curve, value.source)? {
                Some(id) => CurveSource::Reference(CurveId(id)),
                None => CurveSource::Inline(curve_from_points(value.points)),
            },
        ),
        SequenceEffectParamValue::Gradient { value } => EffectParamValue::Gradient(
            match library_identity(session, owner, SourceObjectKind::Gradient, value.source)? {
                Some(id) => GradientSource::Reference(GradientId(id)),
                None => GradientSource::Inline(gradient_from_stops(value.stops)?),
            },
        ),
        SequenceEffectParamValue::IntArray { values } => EffectParamValue::Array(
            values
                .into_iter()
                .map(|value| EffectParamValue::Int(value as i32))
                .collect(),
        ),
        SequenceEffectParamValue::FloatArray { values } => {
            EffectParamValue::Array(values.into_iter().map(EffectParamValue::Float).collect())
        }
        SequenceEffectParamValue::BoolArray { values } => {
            EffectParamValue::Array(values.into_iter().map(EffectParamValue::Bool).collect())
        }
        SequenceEffectParamValue::ColorArray { values } => EffectParamValue::Array(
            values
                .into_iter()
                .map(|value| parse_color(&value).map(EffectParamValue::Color))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        SequenceEffectParamValue::CurveArray { values } => EffectParamValue::Array(
            values
                .into_iter()
                .map(|value| {
                    effect_param_value_from_gui(
                        session,
                        owner,
                        SequenceEffectParamValue::Curve { value },
                    )
                })
                .collect::<Result<Vec<_>, _>>()?,
        ),
        SequenceEffectParamValue::GradientArray { values } => EffectParamValue::Array(
            values
                .into_iter()
                .map(|value| {
                    effect_param_value_from_gui(
                        session,
                        owner,
                        SequenceEffectParamValue::Gradient { value },
                    )
                })
                .collect::<Result<Vec<_>, _>>()?,
        ),
    })
}

fn library_identity(
    session: &mut ProjectSession,
    owner: &SourceIdentity,
    kind: SourceObjectKind,
    source: SequenceLibrarySource,
) -> Result<Option<SourceIdentity>, GuiMutationError> {
    let SequenceLibrarySource::Library {
        module_id,
        path,
        object_key,
        ..
    } = source
    else {
        return Ok(None);
    };
    let id = source_identity_from_gui(&module_id, &path, &object_key)?;
    donder_project_io::link_reusable_source(session, owner.document_id(), kind, &id)
        .map_err(|error| GuiMutationError::Blocked(error.to_string()))?;
    Ok(Some(id))
}

pub(super) fn automation_binding_value_at(
    clip: &AutomationClip,
    mapping: &AutomationMapping,
    seconds: f32,
) -> Result<EffectParamValue, GuiMutationError> {
    automation_value_at(clip, mapping, seconds)
        .map(|value| match value {
            AutomationValue::Int(value) => EffectParamValue::Int(value),
            AutomationValue::Float(value) => EffectParamValue::Float(value),
            AutomationValue::Bool(value) => EffectParamValue::Bool(value),
            AutomationValue::Enum(value) => EffectParamValue::Enum(value.clone()),
            AutomationValue::Curve(value) => EffectParamValue::Curve(CurveSource::Inline(value)),
        })
        .ok_or_else(|| {
            GuiMutationError::Invalid("Enum automation mapping has no values.".to_string())
        })
}

pub(super) fn default_automation_curve() -> Curve {
    Curve {
        points: vec![
            CurvePoint {
                position: 0.0,
                value: 0.0,
            },
            CurvePoint {
                position: 1.0,
                value: 1.0,
            },
        ],
    }
}

pub(super) fn curve_from_points(points: Vec<SequenceCurvePoint>) -> Curve {
    Curve {
        points: points
            .into_iter()
            .map(|point| CurvePoint {
                position: point.time,
                value: point.value,
            })
            .collect(),
    }
}

pub(super) fn curve_points(curve: &Curve) -> Vec<SequenceCurvePoint> {
    curve
        .points
        .iter()
        .map(|point| SequenceCurvePoint {
            time: point.position,
            value: point.value,
        })
        .collect()
}

pub(super) fn gradient_stops(gradient: &Gradient) -> Vec<SequenceGradientStop> {
    gradient
        .stops
        .iter()
        .map(|stop| SequenceGradientStop {
            time: stop.position,
            value: stop.color.to_hex(),
        })
        .collect()
}

pub(super) fn gradient_from_stops(
    stops: Vec<SequenceGradientStop>,
) -> Result<Gradient, GuiMutationError> {
    Ok(Gradient {
        stops: stops
            .into_iter()
            .map(|stop| {
                Ok(GradientStop {
                    position: stop.time,
                    color: parse_color(&stop.value)?,
                })
            })
            .collect::<Result<Vec<_>, GuiMutationError>>()?,
    })
}

pub(super) fn parse_color(value: &str) -> Result<Color, GuiMutationError> {
    Color::from_hex(value)
        .ok_or_else(|| GuiMutationError::Invalid(format!("Invalid color `{value}`.")))
}

pub(super) fn domain_point3_meters(point: Point3Meters) -> Point3 {
    Point3 {
        x: Distance::from_meters(point.x_meters),
        y: Distance::from_meters(point.y_meters),
        z: Distance::from_meters(point.z_meters),
    }
}

pub(super) fn rotation3_degrees(rotation: Rotation3Degrees) -> DomainRotation3 {
    DomainRotation3 {
        x: rotation.x_degrees,
        y: rotation.y_degrees,
        z: rotation.z_degrees,
    }
}

pub(super) fn scale3(scale: Scale3) -> DomainScale3 {
    DomainScale3 {
        x: scale.x,
        y: scale.y,
        z: scale.z,
    }
}

pub fn point3_meters(point: Point3) -> Point3Meters {
    Point3Meters {
        x_meters: point.x.as_meters_f32(),
        y_meters: point.y.as_meters_f32(),
        z_meters: point.z.as_meters_f32(),
    }
}

use std::fs;

use camino::Utf8PathBuf;
use donder_language::{Distance, Point3, Rotation3 as DomainRotation3, Scale3 as DomainScale3};
use donder_model::SourceIdentity;
use donder_model::{
    AssetId, AutomationClip, CompositionGraphNode, CompositionGraphNodeId,
    CompositionGraphNodeKind, EffectGraphEdge, GraphNodePosition, GraphPortId, MarkCollection,
    MarkCollectionKey, SequenceLayerId, automation_value_at,
};
use donder_model::{
    CurveId, CurveSource, EffectInst, EffectParamValue, EffectScope, GradientId, GradientSource,
};
use donder_model::{FixtureInstanceId, FixtureTarget as DomainFixtureTarget, LayoutId};
use donder_model::{OperatorDefinitionId, OperatorPortCardinality, OperatorRef};
use donder_project_io::{ProjectSession, ReferencedAsset, SourceObjectKind};
use donder_runtime_types::Identifier;
use donder_runtime_types::{AutomationMapping, AutomationValue};
use donder_runtime_types::{Color, Curve, CurvePoint, Gradient, GradientStop};

use super::GuiMutationError;
use crate::dto::{
    FixtureTarget, Point3Meters, Rotation3Degrees, Scale3, SequenceCurvePoint,
    SequenceEffectParamValue, SequenceEffectScope, SequenceGradientStop, SequenceGraphOperator,
    SequenceLibrarySource,
};

pub(super) fn create_object_document(
    session: &mut ProjectSession,
    kind: donder_project_io::SourceObjectKind,
    name: &str,
    directory: &str,
) -> Result<donder_model::SourceIdentity, GuiMutationError> {
    let key = donder_language::object_name(name).as_str().to_string();
    for index in 1_u32.. {
        let stem = if index == 1 {
            key.clone()
        } else {
            format!("{key}_{index}")
        };
        let path = camino::Utf8PathBuf::from(format!(
            "{directory}/{stem}{}",
            donder_language::data::DATA_DOCUMENT_SUFFIX
        ));
        let document = session.source.project_document(path.clone());
        if session.source.documents.contains_key(&document)
            || session.source.project_root().join(&path).exists()
        {
            continue;
        }
        return session
            .source
            .add_data_document(path, vec![(kind, stem)])
            .map_err(GuiMutationError::Invalid)?
            .into_iter()
            .next()
            .ok_or_else(|| GuiMutationError::Invalid("New document has no object.".into()));
    }
    Err(GuiMutationError::Invalid(
        "No source document names remain.".into(),
    ))
}

pub fn object_identity_from_gui(
    reference: &crate::dto::GuiObjectRef,
) -> Result<donder_model::ObjectIdentity, GuiMutationError> {
    let root =
        source_identity_from_gui(&reference.module_id, &reference.path, &reference.object_key)?;
    reference.owned_path.iter().try_fold(
        root.into(),
        |address: donder_model::ObjectIdentity, step| {
            Ok(address.owned(step.try_into().map_err(GuiMutationError::Invalid)?))
        },
    )
}
