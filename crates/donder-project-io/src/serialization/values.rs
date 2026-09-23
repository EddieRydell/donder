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

pub(super) fn transform_value(transform: &FixtureTransform) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(string_value("position"), point_value(&transform.position)?);
    value.insert(
        string_value("rotation"),
        rotation_value(&transform.rotation)?,
    );
    value.insert(string_value("scale"), scale_value(&transform.scale)?);
    Ok(Value::Mapping(value))
}

pub(super) fn point_value(point: &Point3) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(
        string_value("x"),
        serialized_value(f64::from(point.x.micrometers) / 1_000_000.0)?,
    );
    value.insert(
        string_value("y"),
        serialized_value(f64::from(point.y.micrometers) / 1_000_000.0)?,
    );
    value.insert(
        string_value("z"),
        serialized_value(f64::from(point.z.micrometers) / 1_000_000.0)?,
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

pub(super) fn fixture_target_value(
    session: &ProjectSession,
    from: &DocumentId,
    target: &FixtureTarget,
) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(
        string_value("layout"),
        string_value(&write_source_reference(
            session,
            from,
            SourceObjectKind::Layout,
            &target.layout.0,
        )?),
    );
    value.insert(string_value("fixture"), serialized_value(target.fixture.0)?);
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
use donder_language::fixture::FixtureTransform;
use donder_language::identity::DocumentId;
use donder_language::layout::FixtureTarget;
use donder_language::values::{Curve, Gradient, Point3, Rotation3, Scale3};
use yaml_serde::{Mapping, Value};

use super::ProjectSession;
use crate::ExportProjectError;
use crate::source::SourceObjectKind;
