//! Closed source mappings. Successful parsing always checks every authored key.
use std::cell::RefCell;
use std::collections::BTreeSet;

use camino::Utf8Path;
use yaml_serde::{Mapping, Value};

use crate::LoadProjectError;
use crate::diagnostics::{source_range_for_field_value, source_range_for_value};

pub(crate) struct MappingReader<'a> {
    path: &'a Utf8Path,
    value: &'a Value,
    mapping: &'a Mapping,
    consumed: RefCell<BTreeSet<String>>,
}

pub(crate) fn parse_mapping<T>(
    path: &Utf8Path,
    value: &Value,
    label: &str,
    parse: impl FnOnce(&MappingReader<'_>) -> Result<T, LoadProjectError>,
) -> Result<T, LoadProjectError> {
    let mapping = value
        .as_mapping()
        .ok_or_else(|| LoadProjectError::InvalidDocument {
            path: path.to_owned(),
            range: source_range_for_value(path, value),
            message: format!("{label} must be a mapping"),
        })?;
    let reader = MappingReader {
        path,
        value,
        mapping,
        consumed: RefCell::new(BTreeSet::new()),
    };
    let result = parse(&reader);
    // A failed parse may not have reached all legitimate keys yet.
    let output = result?;
    for key in mapping.keys() {
        let Some(key) = key.as_str() else {
            return Err(LoadProjectError::InvalidDocument {
                path: path.to_owned(),
                range: source_range_for_value(path, key),
                message: format!("{label} keys must be strings"),
            });
        };
        if !reader.consumed.borrow().contains(key) {
            return Err(LoadProjectError::InvalidDocument {
                path: path.to_owned(),
                range: source_range_for_field_value(path, value, key),
                message: format!("{label} has an unknown field `{key}`"),
            });
        }
    }
    Ok(output)
}

impl<'a> MappingReader<'a> {
    pub(crate) fn optional(&self, key: &str) -> Option<&'a Value> {
        self.consumed.borrow_mut().insert(key.to_owned());
        self.mapping.get(Value::String(key.to_owned()))
    }

    pub(crate) fn required(&self, key: &str) -> Result<&'a Value, LoadProjectError> {
        self.optional(key)
            .ok_or_else(|| LoadProjectError::InvalidDocument {
                path: self.path.to_owned(),
                range: source_range_for_value(self.path, self.value),
                message: format!("missing field `{key}`"),
            })
    }

    fn wrong_type(&self, key: &str, kind: &str) -> LoadProjectError {
        LoadProjectError::InvalidDocument {
            path: self.path.to_owned(),
            range: source_range_for_field_value(self.path, self.value, key),
            message: format!("field `{key}` must be {kind}"),
        }
    }

    pub(crate) fn string(&self, key: &str) -> Result<&'a str, LoadProjectError> {
        self.required(key)?
            .as_str()
            .ok_or_else(|| self.wrong_type(key, "a string"))
    }
    pub(crate) fn u32(&self, key: &str) -> Result<u32, LoadProjectError> {
        self.required(key)?
            .as_u64()
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| self.wrong_type(key, "a u32"))
    }
    pub(crate) fn i32(&self, key: &str) -> Result<i32, LoadProjectError> {
        self.required(key)?
            .as_i64()
            .and_then(|v| i32::try_from(v).ok())
            .ok_or_else(|| self.wrong_type(key, "an integer"))
    }
    pub(crate) fn f32(&self, key: &str) -> Result<f32, LoadProjectError> {
        self.required(key)?
            .as_f64()
            .map(|v| v as f32)
            .ok_or_else(|| self.wrong_type(key, "a number"))
    }
    pub(crate) fn bool(&self, key: &str) -> Result<bool, LoadProjectError> {
        self.required(key)?
            .as_bool()
            .ok_or_else(|| self.wrong_type(key, "a bool"))
    }
    pub(crate) fn sequence(&self, key: &str) -> Result<&'a Vec<Value>, LoadProjectError> {
        self.required(key)?
            .as_sequence()
            .ok_or_else(|| self.wrong_type(key, "a sequence"))
    }
    pub(crate) fn optional_sequence(
        &self,
        key: &str,
    ) -> Result<Option<&'a Vec<Value>>, LoadProjectError> {
        self.optional(key)
            .map(|v| {
                v.as_sequence()
                    .ok_or_else(|| self.wrong_type(key, "a sequence"))
            })
            .transpose()
    }
    pub(crate) fn strings(&self, key: &str) -> Result<Vec<String>, LoadProjectError> {
        self.sequence(key)?
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| LoadProjectError::InvalidDocument {
                        path: self.path.to_owned(),
                        range: source_range_for_value(self.path, v),
                        message: format!("field `{key}` values must be strings"),
                    })
            })
            .collect()
    }
    /// Dynamic names are deliberate. Every entry is passed through the parser.
    pub(crate) fn dictionary<T>(
        &self,
        key: &str,
        mut parse: impl FnMut(&'a str, &'a Value) -> Result<T, LoadProjectError>,
    ) -> Result<Vec<T>, LoadProjectError> {
        let Some(value) = self.optional(key) else {
            return Ok(Vec::new());
        };
        let mapping = value
            .as_mapping()
            .ok_or_else(|| self.wrong_type(key, "a mapping"))?;
        mapping
            .iter()
            .map(|(name, value)| {
                let name = name
                    .as_str()
                    .ok_or_else(|| LoadProjectError::InvalidDocument {
                        path: self.path.to_owned(),
                        range: source_range_for_value(self.path, name),
                        message: format!("field `{key}` keys must be strings"),
                    })?;
                parse(name, value)
            })
            .collect()
    }
}
