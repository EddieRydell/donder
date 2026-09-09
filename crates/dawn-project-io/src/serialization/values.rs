pub(super) use crate::imports::write_source_reference;
pub(super) fn curve_value(curve: &Curve) -> Result<Value, ExportProjectError> {
    let mut value = typed_object("curve");
    value.insert(
        string_value("points"),
        Value::Sequence(
            curve
                .points
                .iter()
                .map(|point| {
                    serialized_value(crate::schema::CurvePoint {
                        position: point.position,
                        value: point.value,
                    })
                })
                .collect::<Result<Vec<_>, ExportProjectError>>()?,
        ),
    );
    Ok(Value::Mapping(value))
}

pub(super) fn gradient_value(gradient: &Gradient) -> Result<Value, ExportProjectError> {
    let mut value = typed_object("gradient");
    value.insert(
        string_value("stops"),
        Value::Sequence(
            gradient
                .stops
                .iter()
                .map(|stop| {
                    serialized_value(crate::schema::GradientStop {
                        position: stop.position,
                        color: stop.color.to_hex(),
                    })
                })
                .collect::<Result<Vec<_>, ExportProjectError>>()?,
        ),
    );
    Ok(Value::Mapping(value))
}

pub(super) fn geometry_value(geometry: &PropGeometry) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    match geometry {
        PropGeometry::Points { points } => {
            value.insert(string_value("type"), Value::String("points".to_string()));
            value.insert(
                string_value("points"),
                Value::Sequence(
                    points
                        .iter()
                        .map(point_value)
                        .collect::<Result<Vec<_>, _>>()?,
                ),
            );
        }
        PropGeometry::Lines {
            points,
            point_count,
        } => {
            value.insert(string_value("type"), Value::String("lines".to_string()));
            value.insert(
                string_value("points"),
                Value::Sequence(
                    points
                        .iter()
                        .map(point_value)
                        .collect::<Result<Vec<_>, _>>()?,
                ),
            );
            value.insert(string_value("point_count"), serialized_value(*point_count)?);
        }
        PropGeometry::Arc {
            center,
            radius,
            start_degrees,
            end_degrees,
            point_count,
        } => {
            value.insert(string_value("type"), Value::String("arc".to_string()));
            value.insert(string_value("center"), point_value(center)?);
            value.insert(
                string_value("radius"),
                serialized_value(radius.as_meters_f32())?,
            );
            value.insert(
                string_value("startDegrees"),
                serialized_value(*start_degrees)?,
            );
            value.insert(string_value("endDegrees"), serialized_value(*end_degrees)?);
            value.insert(string_value("point_count"), serialized_value(*point_count)?);
        }
    }
    Ok(Value::Mapping(value))
}

pub(super) fn transform_value(prop: &PropInstance) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(string_value("position"), point_value(&prop.position)?);
    value.insert(string_value("rotation"), rotation_value(&prop.rotation)?);
    value.insert(string_value("scale"), scale_value(&prop.scale)?);
    Ok(Value::Mapping(value))
}

pub(super) fn point_value(point: &Point3) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(
        string_value("x"),
        serialized_value(point.x.as_meters_f32())?,
    );
    value.insert(
        string_value("y"),
        serialized_value(point.y.as_meters_f32())?,
    );
    value.insert(
        string_value("z"),
        serialized_value(point.z.as_meters_f32())?,
    );
    Ok(Value::Mapping(value))
}

pub(super) fn rotation_value(rotation: &Rotation3) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(string_value("x"), serialized_value(rotation.x)?);
    value.insert(string_value("y"), serialized_value(rotation.y)?);
    value.insert(string_value("z"), serialized_value(rotation.z)?);
    Ok(Value::Mapping(value))
}

pub(super) fn scale_value(scale: &Scale3) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(string_value("x"), serialized_value(scale.x)?);
    value.insert(string_value("y"), serialized_value(scale.y)?);
    value.insert(string_value("z"), serialized_value(scale.z)?);
    Ok(Value::Mapping(value))
}

pub(super) fn element_selection_value(
    session: &ProjectSession,
    from_document: &DocumentId,
    target: &ElementSelection,
) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(
        string_value("tree"),
        Value::String(write_source_reference(
            session,
            from_document,
            SourceObjectKind::ElementTree,
            &target.tree.0,
        )?),
    );
    value.insert(string_value("node"), serialized_value(target.node.0)?);
    if let Some(range) = target.cells {
        let mut cells = Mapping::new();
        cells.insert(string_value("start"), serialized_value(range.start)?);
        cells.insert(string_value("count"), serialized_value(range.count)?);
        value.insert(string_value("cells"), Value::Mapping(cells));
    }
    Ok(Value::Mapping(value))
}

pub(super) fn typed_object(object_type: &str) -> Mapping {
    let mut value = Mapping::new();
    value.insert(string_value("type"), Value::String(object_type.to_string()));
    value
}

pub(super) fn string_value(value: &str) -> Value {
    Value::String(value.to_string())
}

pub(super) fn serialized_value<T: serde::Serialize>(value: T) -> Result<Value, ExportProjectError> {
    yaml_serde::to_value(value).map_err(|source| ExportProjectError::Serialize {
        path: Utf8PathBuf::from("<sync>"),
        source,
    })
}

pub(super) fn microseconds_string(microseconds: u128) -> String {
    format!(
        "{}s",
        dur::Duration::from_micros(microseconds).as_secs_dec()
    )
}

use camino::Utf8PathBuf;
use dawn_language::element::ElementSelection;
use dawn_language::identity::DocumentId;
use dawn_language::preview::{PropGeometry, PropInstance};
use dawn_language::values::{Curve, Gradient, Point3, Rotation3, Scale3};
use yaml_serde::{Mapping, Value};

use super::ProjectSession;
use crate::ExportProjectError;
use crate::source::SourceObjectKind;
