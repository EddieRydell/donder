//! Partial reads for source indexing and recovery. These do not accept a project.
use super::parse::mapping;
use crate::LoadProjectError;
use crate::diagnostics::{source_range_for_field_value, source_range_for_value};
use camino::Utf8Path;
use yaml_serde::Value;
pub(crate) fn required_field<'a>(
    path: &Utf8Path,
    value: &'a Value,
    key: &str,
) -> Result<&'a Value, LoadProjectError> {
    mapping(value)
        .and_then(|mapping| mapping.get(Value::String(key.to_string())))
        .ok_or_else(|| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range: source_range_for_value(path, value),
            message: format!("missing field `{key}`"),
        })
}

pub(crate) fn optional_field<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    mapping(value).and_then(|mapping| mapping.get(Value::String(key.to_string())))
}

pub(crate) fn optional_sequence<'a>(
    path: &Utf8Path,
    value: &'a Value,
    key: &str,
) -> Result<Option<&'a Vec<Value>>, LoadProjectError> {
    optional_field(value, key)
        .map(|field| {
            field
                .as_sequence()
                .ok_or_else(|| LoadProjectError::InvalidDocument {
                    path: path.to_path_buf(),
                    range: source_range_for_field_value(path, value, key),
                    message: format!("field `{key}` must be a sequence"),
                })
        })
        .transpose()
}

pub(crate) fn sequence_values<'a>(
    path: &Utf8Path,
    value: &'a Value,
    key: &str,
) -> Result<&'a Vec<Value>, LoadProjectError> {
    required_field(path, value, key)?
        .as_sequence()
        .ok_or_else(|| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range: source_range_for_field_value(path, value, key),
            message: format!("field `{key}` must be a sequence"),
        })
}

pub(crate) fn string_field<'a>(
    path: &Utf8Path,
    value: &'a Value,
    key: &str,
) -> Result<&'a str, LoadProjectError> {
    required_field(path, value, key)?
        .as_str()
        .ok_or_else(|| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range: source_range_for_field_value(path, value, key),
            message: format!("field `{key}` must be a string"),
        })
}

pub(crate) fn u32_field(
    path: &Utf8Path,
    value: &Value,
    key: &str,
) -> Result<u32, LoadProjectError> {
    required_field(path, value, key)?
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range: source_range_for_field_value(path, value, key),
            message: format!("field `{key}` must be a u32"),
        })
}

pub(crate) fn f32_field(
    path: &Utf8Path,
    value: &Value,
    key: &str,
) -> Result<f32, LoadProjectError> {
    required_field(path, value, key)?
        .as_f64()
        .map(|value| value as f32)
        .ok_or_else(|| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range: source_range_for_field_value(path, value, key),
            message: format!("field `{key}` must be a number"),
        })
}

pub(crate) fn bool_field(
    path: &Utf8Path,
    value: &Value,
    key: &str,
) -> Result<bool, LoadProjectError> {
    required_field(path, value, key)?
        .as_bool()
        .ok_or_else(|| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range: source_range_for_field_value(path, value, key),
            message: format!("field `{key}` must be a bool"),
        })
}
