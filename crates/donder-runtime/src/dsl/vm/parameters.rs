//! Bound parameter values in the layout of the program's declared parameters:
//! one 32-bit word per word parameter and one shared resource per resource
//! parameter, each numbered in declaration order (see `ParamStorage`). Equal
//! resources bound anywhere in one sequence share one allocation.
use super::{Arc, PreparedCurve};
use alloc::{boxed::Box, vec::Vec};
use donder_runtime_types::bytecode::{BytecodeProgram, ParamStorage, ParameterKind};
use donder_runtime_types::{Curve, Gradient, Identifier, Marks, Type, Value};

#[derive(Clone, Debug, Default, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct BoundParams {
    pub(super) words: Box<[u32]>,
    pub(super) resources: Box<[ResourceParam]>,
}

#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(super) enum ResourceParam {
    Curve(Arc<PreparedCurve>),
    Gradient(Arc<Gradient>),
    Marks(Arc<Marks>),
    Array(Arc<[Value]>),
}

impl BoundParams {
    /// Materialize an invocation's checked values for `program`.
    pub(crate) fn from_validated(
        program: &BytecodeProgram,
        params: &donder_runtime_types::BoundParams,
        cache: &mut DslBindCache,
    ) -> Self {
        Self::from_values(
            &program.enums,
            program.params.iter().zip(params.values()),
            cache,
        )
    }

    /// Materialize type-checked values in declaration order; enum values
    /// become their index in `enums`.
    pub(crate) fn from_values<'a>(
        enums: &[Identifier],
        values: impl IntoIterator<Item = (&'a Type, &'a Value)>,
        cache: &mut DslBindCache,
    ) -> Self {
        let mut words = Vec::new();
        let mut resources = Vec::new();
        for (ty, value) in values {
            match ParameterKind::for_type(ty).storage() {
                None => {}
                Some(ParamStorage::Word) => words.push(word(ty, value, enums)),
                Some(ParamStorage::Resource) => resources.push(cache.resource(value)),
            }
        }
        Self {
            words: words.into(),
            resources: resources.into(),
        }
    }

    /// Whether these values have the layout of `program`'s parameters: a word
    /// per word parameter and a resource of each resource parameter's kind.
    pub(crate) fn fits(&self, program: &BytecodeProgram) -> bool {
        let mut words = 0;
        let mut resources = self.resources.iter();
        for ty in &program.params {
            let kind = ParameterKind::for_type(ty);
            match kind.storage() {
                None => {}
                Some(ParamStorage::Word) => words += 1,
                Some(ParamStorage::Resource) => {
                    let fits = matches!(
                        (kind, resources.next()),
                        (ParameterKind::Curve, Some(ResourceParam::Curve(_)))
                            | (ParameterKind::Gradient, Some(ResourceParam::Gradient(_)))
                            | (ParameterKind::Marks, Some(ResourceParam::Marks(_)))
                            | (ParameterKind::Array, Some(ResourceParam::Array(_)))
                    );
                    if !fits {
                        return false;
                    }
                }
            }
        }
        words == self.words.len() && resources.next().is_none()
    }

    #[cfg(test)]
    pub(crate) fn bind_values(
        types: &[Type],
        values: Vec<Value>,
        cache: &mut DslBindCache,
    ) -> Result<Self, super::RuntimeError> {
        let accepted = donder_runtime_types::BoundParams::bind_values(types, values)
            .map_err(|error| super::RuntimeError::new(error.message))?;
        Ok(Self::from_values(
            &[],
            accepted.types().iter().zip(accepted.values()),
            cache,
        ))
    }

    /// Conservative load-time budget for an automated copy whose curve
    /// windows are the resources in `windows`, each up to its point count.
    pub(crate) fn automation_storage_estimate(
        &self,
        windows: impl IntoIterator<Item = (usize, usize)>,
    ) -> Option<usize> {
        let mut bytes = self
            .words
            .len()
            .checked_mul(size_of::<u32>())?
            .checked_add(
                self.resources
                    .len()
                    .checked_mul(size_of::<ResourceParam>())?,
            )?;
        for (resource, points) in windows {
            let ResourceParam::Curve(curve) = self.resources.get(resource)? else {
                return None;
            };
            // Three detached shared allocations; forward samples use the raw points.
            let points = points.max(curve.raw.points.len()).max(1);
            bytes = bytes.checked_add(
                points
                    .checked_mul(
                        size_of::<donder_runtime_types::CurvePoint>()
                            + size_of::<super::CrossingSegment>(),
                    )?
                    .checked_add(
                        size_of::<PreparedCurve>()
                            + size_of::<Curve>()
                            + size_of::<super::PreparedCurveCrossings>()
                            + 6 * size_of::<usize>(),
                    )?,
            )?;
        }
        Some(bytes)
    }
}

/// A word parameter's value.
fn word(ty: &Type, value: &Value, enums: &[Identifier]) -> u32 {
    match (ty, value) {
        (Type::Float, Value::Int(value)) => (*value as f32).to_bits(),
        (_, Value::Float(value)) => value.to_bits(),
        (_, Value::Int(value)) => *value as u32,
        (_, Value::Bool(value)) => u32::from(*value),
        // `strip::word_color` reads it back.
        (_, Value::Color(color)) => u32::from_le_bytes([color.red, color.green, color.blue, 0]),
        (_, Value::Enum(name)) => enums
            .iter()
            .position(|option| option == name)
            .map_or(-1, |index| index as i32) as u32,
        _ => unreachable!("word parameters hold scalar values"),
    }
}

/// Resources bound so far, so that equal values share one allocation.
#[derive(Debug, Default)]
pub(crate) struct DslBindCache {
    curves: Vec<(Arc<Curve>, Arc<PreparedCurve>)>,
    gradients: Vec<Arc<Gradient>>,
    tracks: Vec<Arc<[u32]>>,
    marks: Vec<Arc<Marks>>,
    arrays: Vec<Arc<[Value]>>,
}

impl DslBindCache {
    fn resource(&mut self, value: &Value) -> ResourceParam {
        match value {
            Value::Curve(curve) => ResourceParam::Curve(self.curve(curve)),
            Value::Gradient(gradient) => {
                ResourceParam::Gradient(intern(&mut self.gradients, gradient, Gradient::same))
            }
            Value::Marks(marks) => ResourceParam::Marks(self.marks(marks)),
            Value::Array(items) => ResourceParam::Array(intern(&mut self.arrays, items, |a, b| {
                a.len() == b.len() && a.iter().zip(b).all(|(a, b)| a.same(b))
            })),
            _ => unreachable!("resource parameters hold resources"),
        }
    }

    fn curve(&mut self, raw: &Arc<Curve>) -> Arc<PreparedCurve> {
        if let Some((_, prepared)) = self
            .curves
            .iter()
            .find(|(existing, _)| Arc::ptr_eq(existing, raw) || existing.same(raw))
        {
            return Arc::clone(prepared);
        }
        let prepared = Arc::new(PreparedCurve::new(Arc::clone(raw)));
        self.curves.push((Arc::clone(raw), Arc::clone(&prepared)));
        prepared
    }

    fn marks(&mut self, marks: &Arc<Marks>) -> Arc<Marks> {
        let track = intern(&mut self.tracks, marks.track(), |a, b| a == b);
        let window = if Arc::ptr_eq(&track, marks.track()) {
            Arc::clone(marks)
        } else {
            Arc::new(marks.with_track(track))
        };
        intern(&mut self.marks, &window, |a, b| a == b)
    }
}

fn intern<T: ?Sized>(
    pool: &mut Vec<Arc<T>>,
    value: &Arc<T>,
    same: impl Fn(&T, &T) -> bool,
) -> Arc<T> {
    if let Some(existing) = pool
        .iter()
        .find(|existing| Arc::ptr_eq(existing, value) || same(existing, value))
    {
        return Arc::clone(existing);
    }
    pool.push(Arc::clone(value));
    Arc::clone(value)
}
