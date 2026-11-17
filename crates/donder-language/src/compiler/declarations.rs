//! Authoring declarations and named parameter resolution.
//! Playback programs receive positional, typed values rather than source names.
use donder_runtime_types::AutomationMapping;
use donder_runtime_types::{BindingError, Identifier, Type, Value};

#[derive(Clone, Debug, PartialEq)]
pub struct OperatorInputDecl {
    pub name: Identifier,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParamDecl {
    pub name: Identifier,
    pub ty: Type,
    /// Present exactly for `int`, `float`, `curve` and `array<curve>` params.
    pub range: Option<ParamRange>,
    pub default: Option<Value>,
    /// Shown in the inspector beside the parameter.
    pub description: Option<String>,
    /// For an enum parameter, each option's fields, in option order: an
    /// option without fields has none. Empty for every other type. A field
    /// is an int, float, bool, color or plain enum with a default.
    pub option_fields: Vec<Vec<ParamDecl>>,
}

/// Inclusive declared range. A curve param's range bounds its point values,
/// and an `array<curve>` param's range bounds every item's point values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ParamRange {
    Int { min: i32, max: i32 },
    Float { min: f32, max: f32 },
}

impl ParamRange {
    /// Whether this range is well formed and fits a param of `ty`.
    pub(crate) fn fits(&self, ty: &Type) -> bool {
        match (self, ty) {
            (Self::Int { min, max }, Type::Int) => min <= max,
            (Self::Float { min, max }, Type::Float | Type::Curve) => {
                min.is_finite() && max.is_finite() && min <= max
            }
            (Self::Float { .. }, Type::Array(item)) if **item == Type::Curve => self.fits(item),
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
            (Self::Float { .. }, Value::Array(items)) => {
                items.iter().all(|item| self.contains(item))
            }
            _ => false,
        }
    }
}

impl ParamDecl {
    /// Every type but an enum whose options carry fields.
    pub fn supports_automation(&self) -> bool {
        matches!(
            self.ty,
            Type::Float | Type::Int | Type::Bool | Type::Enum(_) | Type::Curve
        ) && !self.has_fields()
    }

    /// Whether an option of this enum parameter carries fields.
    pub fn has_fields(&self) -> bool {
        self.option_fields.iter().any(|fields| !fields.is_empty())
    }

    /// The fields of option `option`; none for an unknown option or another
    /// type.
    pub fn fields_of(&self, option: &Identifier) -> &[ParamDecl] {
        match &self.ty {
            Type::Enum(options) => options
                .iter()
                .position(|candidate| candidate == option)
                .and_then(|index| self.option_fields.get(index))
                .map_or(&[], Vec::as_slice),
            _ => &[],
        }
    }

    /// The value has this param's type and lies within its declared range.
    pub(crate) fn accepts_value(&self, value: &Value) -> bool {
        self.ty.accepts_value(value) && self.range.is_none_or(|range| range.contains(value))
    }

    /// Automation maps its normalized curve onto the declared range or options.
    pub fn automation_mapping(&self) -> Option<AutomationMapping> {
        if self.has_fields() {
            return None;
        }
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

/// Every value of the declared type lies within its declared range.
pub(super) fn check_ranges<'a>(
    declarations: impl Iterator<Item = &'a ParamDecl>,
    values: &[Value],
) -> Result<(), BindingError> {
    for (param, value) in declarations.zip(values) {
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

/// A program's parameters: the declared ones, then each field of every enum
/// parameter's options, in declaration, option and field order. Playback
/// reads a field as a parameter of its own.
pub(crate) fn program_params<'a>(
    params: impl Iterator<Item = &'a ParamDecl> + Clone,
) -> impl Iterator<Item = &'a ParamDecl> {
    params
        .clone()
        .chain(params.flat_map(|param| param.option_fields.iter().flatten()))
}

/// The program's values: `values`, one per declared parameter, then each
/// option field's value. The chosen option of an enum parameter takes its
/// fields from `chosen`, keyed by the parameter's index, in field order;
/// every other option's fields hold their defaults.
pub fn program_values(
    params: &[ParamDecl],
    mut values: Vec<Value>,
    chosen: &[(usize, Vec<Value>)],
) -> Vec<Value> {
    let mut fields = Vec::new();
    for (index, param) in params.iter().enumerate() {
        let Type::Enum(options) = &param.ty else {
            continue;
        };
        for (option, declared) in options.iter().zip(&param.option_fields) {
            let picked = matches!(values.get(index), Some(Value::Enum(value)) if value == option);
            match chosen.iter().find(|(param, _)| *param == index) {
                Some((_, values)) if picked => fields.extend(values.iter().cloned()),
                _ => fields.extend(declared.iter().map(|field| {
                    field
                        .default
                        .clone()
                        .unwrap_or_else(|| unreachable!("every field has a default"))
                })),
            }
        }
    }
    values.extend(fields);
    values
}
