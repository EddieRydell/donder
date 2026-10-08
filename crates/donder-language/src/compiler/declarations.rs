//! Authoring declarations and named parameter resolution.
//! Playback programs receive positional, typed values rather than source names.
use donder_runtime_types::AutomationMapping;
use donder_runtime_types::{BindingError, BoundParams, Identifier, Type, Value};

#[derive(Clone, Debug, PartialEq)]
pub struct OperatorInputDecl {
    pub name: Identifier,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParamDecl {
    pub name: Identifier,
    pub ty: Type,
    /// Present exactly for `int`, `float`, and `curve` params.
    pub range: Option<ParamRange>,
    pub default: Option<Value>,
    /// Shown in the inspector beside the parameter.
    pub description: Option<String>,
}

/// Inclusive declared range. A curve param's range bounds its point values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ParamRange {
    Int { min: i32, max: i32 },
    Float { min: f32, max: f32 },
}

impl ParamRange {
    /// Whether this range is well formed and fits a param of `ty`.
    pub fn fits(&self, ty: &Type) -> bool {
        match (self, ty) {
            (Self::Int { min, max }, Type::Int) => min <= max,
            (Self::Float { min, max }, Type::Float | Type::Curve) => {
                min.is_finite() && max.is_finite() && min <= max
            }
            _ => false,
        }
    }

    fn contains_float(&self, value: f32) -> bool {
        match *self {
            Self::Int { min, max } => (min as f32..=max as f32).contains(&value),
            Self::Float { min, max } => (min..=max).contains(&value),
        }
    }

    fn contains(&self, value: &Value) -> bool {
        match (self, value) {
            (Self::Int { min, max }, Value::Int(value)) => (*min..=*max).contains(value),
            (Self::Float { .. }, Value::Int(value)) => self.contains_float(*value as f32),
            (Self::Float { .. }, Value::Float(value)) => self.contains_float(*value),
            (Self::Float { .. }, Value::Curve(curve)) => curve
                .points
                .iter()
                .all(|point| self.contains_float(point.value)),
            _ => false,
        }
    }
}

impl ParamDecl {
    pub fn supports_automation(&self) -> bool {
        matches!(
            self.ty,
            Type::Float | Type::Int | Type::Bool | Type::Enum(_) | Type::Curve
        )
    }

    /// The value has this param's type and lies within its declared range.
    pub fn accepts_value(&self, value: &Value) -> bool {
        self.ty.accepts_value(value) && self.range.is_none_or(|range| range.contains(value))
    }

    /// Automation maps its normalized curve onto the declared range or options.
    pub fn automation_mapping(&self) -> Option<AutomationMapping> {
        Some(match (&self.ty, self.range) {
            (Type::Int, Some(ParamRange::Int { min, max })) => AutomationMapping::Int { min, max },
            (Type::Float, Some(ParamRange::Float { min, max })) => {
                AutomationMapping::Float { min, max }
            }
            (Type::Curve, Some(ParamRange::Float { min, max })) => {
                AutomationMapping::Curve { min, max }
            }
            (Type::Bool, None) => AutomationMapping::Bool,
            (Type::Enum(options), None) => AutomationMapping::Enum {
                values: options.clone(),
            },
            _ => return None,
        })
    }
}

/// Resolve authoring names/defaults and bind their validated positional values.
pub fn bind_params<'p, P>(
    declarations: &[ParamDecl],
    params: P,
) -> Result<BoundParams, BindingError>
where
    P: Clone + IntoIterator<Item = (&'p Identifier, &'p Value)>,
{
    let values = resolve_params(declarations, params)?;
    check_ranges(declarations, &values)?;
    let types: Vec<_> = declarations.iter().map(|param| param.ty.clone()).collect();
    BoundParams::bind_values(&types, values)
}

/// Every value of the declared type lies within its declared range.
pub(super) fn check_ranges(
    declarations: &[ParamDecl],
    values: &[Value],
) -> Result<(), BindingError> {
    for (param, value) in declarations.iter().zip(values) {
        if param.ty.accepts_value(value) && !param.accepts_value(value) {
            return Err(BindingError {
                message: format!(
                    "parameter `{}` is outside its declared range",
                    param.name.as_str()
                ),
            });
        }
    }
    Ok(())
}

pub(super) fn resolve_params<'p, P>(
    declarations: &[ParamDecl],
    params: P,
) -> Result<Vec<Value>, BindingError>
where
    P: Clone + IntoIterator<Item = (&'p Identifier, &'p Value)>,
{
    if let Some(name) = params
        .clone()
        .into_iter()
        .map(|(name, _)| name)
        .find(|name| !declarations.iter().any(|param| param.name == **name))
    {
        return Err(BindingError {
            message: format!("unknown parameter `{}`", name.as_str()),
        });
    }
    declarations
        .iter()
        .map(|param| {
            let value = params
                .clone()
                .into_iter()
                .find(|(name, _)| **name == param.name)
                .map(|(_, value)| value.clone())
                .or_else(|| param.default.clone())
                .ok_or_else(|| BindingError {
                    message: format!("missing required parameter `{}`", param.name.as_str()),
                })?;
            Ok(value)
        })
        .collect()
}
