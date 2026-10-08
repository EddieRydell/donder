mod arrays;
mod context;
pub(crate) use context::NoSignals;
mod automation;
mod parameters;
mod strip;
pub(crate) use automation::AutomationPlan;
pub(crate) use strip::{Pixels, STRIP, Strip, StripSignals, StripWorkspace};

use parameters::{CurveParameter, ParameterAddress, ParameterValues};

use alloc::boxed::Box;
#[cfg(test)]
use alloc::string::String;
use alloc::vec::Vec;
use donder_runtime_types::Shared as Arc;
use donder_runtime_types::{Color, Curve, Gradient, Marks, SampleDuration};
use donder_runtime_types::{Identifier, Type, Value};

#[derive(Clone, Copy, Debug)]
pub(crate) struct RunContext {
    pub progress: f32,
    pub time: SampleDuration,
    pub duration: SampleDuration,
    pub pixel_count: i32,
}

#[cfg(test)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RuntimeError {
    pub message: String,
}

#[cfg(test)]
impl RuntimeError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

#[derive(Clone, Debug, Default, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct BoundParams {
    values: Box<ParameterValues>,
}

impl BoundParams {
    /// Materialize compiler-checked inputs once; no VM-bank validation is repeated here.
    pub(crate) fn from_validated(
        params: &donder_runtime_types::BoundParams,
        cache: &mut DslBindCache,
    ) -> Self {
        Self::from_values(params.types().iter().zip(params.iter_values()), cache)
    }

    #[cfg(test)]
    pub(crate) fn bind_values(
        types: &[Type],
        values: Vec<Value>,
        cache: &mut DslBindCache,
    ) -> Result<Self, RuntimeError> {
        let accepted = donder_runtime_types::BoundParams::bind_values(types, values)
            .map_err(|error| RuntimeError::new(error.message))?;
        Ok(Self::from_validated(&accepted, cache))
    }
    /// Materialize already type-checked values in declaration order. Unlike
    /// `bind_values`, this performs no parameter validation.
    /// Admission of bytecode and externally supplied parameters remains checked.
    pub(crate) fn from_values<'a>(
        values: impl IntoIterator<Item = (&'a Type, Value)>,
        cache: &mut DslBindCache,
    ) -> Self {
        Self {
            values: values
                .into_iter()
                .map(|(ty, value)| (ty, BoundParamValue::from_value(ty, value, cache)))
                .collect::<ParameterValues>()
                .into(),
        }
    }

    /// Materialize an owned value during host preparation or inspection.
    #[cfg(test)]
    fn value(&self, index: usize) -> Result<Value, RuntimeError> {
        let value = self
            .values
            .get(index)
            .ok_or_else(|| RuntimeError::new("invalid parameter slot"))?;
        Ok(value.to_value())
    }

    pub(crate) fn types(&self) -> &[Type] {
        &self.values.types
    }

    /// Conservative load-time budget for the detached automation copy, including
    /// curve windows. This does not allocate or change frame evaluation.
    pub(crate) fn automation_storage_estimate(
        &self,
        bindings: &[crate::signal::PreparedAutomation],
    ) -> Option<usize> {
        let mut bytes = self
            .values
            .len()
            .checked_mul(size_of::<BoundParamValue>() + size_of::<ParameterAddress>())?
            .checked_add(size_of::<ParameterValues>())?;
        for (index, value) in self.values.iter().enumerate() {
            let extra = match value {
                BoundParamValue::Curve(curve) => {
                    let points = bindings
                        .iter()
                        .filter(|binding| usize::from(binding.param_index) == index)
                        .map(|binding| binding.curve.points.len())
                        .max()
                        .unwrap_or(0)
                        .max(curve.raw.points.len())
                        .max(1);
                    // Three detached shared allocations; forward samples use the raw points.
                    points
                        .checked_mul(
                            size_of::<donder_runtime_types::CurvePoint>()
                                + size_of::<CrossingSegment>(),
                        )?
                        .checked_add(
                            size_of::<PreparedCurve>()
                                + size_of::<Curve>()
                                + size_of::<PreparedCurveCrossings>()
                                + 6 * size_of::<usize>(),
                        )?
                }
                _ => 0,
            };
            bytes = bytes.checked_add(extra)?;
        }
        Some(bytes)
    }
}

#[derive(Debug, Default)]
pub(crate) struct DslBindCache {
    curves: Vec<(usize, Arc<PreparedCurve>)>,
}

#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
enum BoundParamValue {
    Void,
    Int(i32),
    Float(f32),
    Bool(bool),
    Color(Color),
    Marks(Arc<Marks>),
    Curve(Arc<PreparedCurve>),
    RawCurve(Arc<Curve>),
    Gradient(Arc<Gradient>),
    Array(Arc<[Value]>),
    Enum(Identifier),
}

impl BoundParamValue {
    fn from_value(ty: &Type, value: Value, cache: &mut DslBindCache) -> Self {
        let value = match (ty, value) {
            (Type::Float, Value::Int(value)) => Value::Float(value as f32),
            (_, value) => value,
        };
        match value {
            Value::Void => Self::Void,
            Value::Int(value) => Self::Int(value),
            Value::Float(value) => Self::Float(value),
            Value::Bool(value) => Self::Bool(value),
            Value::Color(value) => Self::Color(value),
            Value::Marks(value) => Self::Marks(value),
            Value::Curve(value) => Self::Curve(cache.prepared_curve(value)),
            Value::Gradient(value) => Self::Gradient(value),
            Value::Array(value) => Self::Array(value),
            Value::Enum(value) => Self::Enum(value),
        }
    }

    #[cfg(test)]
    fn to_value(&self) -> Value {
        match self {
            Self::Void => Value::Void,
            Self::Int(value) => Value::Int(*value),
            Self::Float(value) => Value::Float(*value),
            Self::Bool(value) => Value::Bool(*value),
            Self::Color(value) => Value::Color(*value),
            Self::Marks(value) => Value::Marks(Arc::clone(value)),
            Self::Curve(value) => Value::Curve(value.raw()),
            Self::RawCurve(value) => Value::Curve(Arc::clone(value)),
            Self::Gradient(value) => Value::Gradient(Arc::clone(value)),
            Self::Array(value) => Value::Array(Arc::clone(value)),
            Self::Enum(value) => Value::Enum(value.clone()),
        }
    }
}

impl DslBindCache {
    fn prepared_curve(&mut self, raw: Arc<Curve>) -> Arc<PreparedCurve> {
        let key = Arc::as_ptr(&raw).cast::<()>() as usize;
        if let Some((_, curve)) = self.curves.iter().find(|(candidate, _)| *candidate == key) {
            return Arc::clone(curve);
        }
        let curve = Arc::new(PreparedCurve::new(raw));
        self.curves.push((key, Arc::clone(&curve)));
        curve
    }
}

#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
struct PreparedCurve {
    raw: Arc<Curve>,
    crossings: Arc<PreparedCurveCrossings>,
}

#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) enum PreparedCurveCrossings {
    Increasing(Vec<CrossingSegment>),
    Decreasing(Vec<CrossingSegment>),
    Mixed(Vec<CrossingSegment>),
}

#[derive(Clone, Copy, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct CrossingSegment {
    position_bias: f32,
    position_scale: f32,
    min_value: f32,
    max_value: f32,
}

impl PreparedCurve {
    fn new(raw: Arc<Curve>) -> Self {
        let crossings = Arc::new(prepare_curve_crossings(&raw));
        Self { raw, crossings }
    }

    fn raw(&self) -> Arc<Curve> {
        Arc::clone(&self.raw)
    }

    fn detached_clone(&self) -> Self {
        Self {
            raw: Arc::new((*self.raw).clone()),
            crossings: Arc::new((*self.crossings).clone()),
        }
    }

    fn reserve_window_capacity(&mut self, point_count: usize) {
        // Empty windows still emit one sampled fallback point.
        let point_count = point_count.max(1);
        let raw = Arc::make_mut(&mut self.raw);
        if raw.points.capacity() < point_count {
            raw.points.reserve_exact(point_count - raw.points.len());
        }
        let crossings = match Arc::make_mut(&mut self.crossings) {
            PreparedCurveCrossings::Increasing(values)
            | PreparedCurveCrossings::Decreasing(values)
            | PreparedCurveCrossings::Mixed(values) => values,
        };
        if crossings.capacity() < point_count {
            crossings.reserve_exact(point_count - crossings.len());
        }
    }

    fn update_window(&mut self, curve: &Curve, min: f32, max: f32, position: f32) {
        donder_runtime_types::curve_window_into(
            Arc::make_mut(&mut self.raw),
            curve,
            min,
            max,
            position,
        );
        prepare_curve_crossings_into(&self.raw, Arc::make_mut(&mut self.crossings));
    }
}

fn prepare_curve_crossings(curve: &Curve) -> PreparedCurveCrossings {
    let mut crossings = PreparedCurveCrossings::Increasing(Vec::with_capacity(curve.points.len()));
    prepare_curve_crossings_into(curve, &mut crossings);
    crossings
}

fn prepare_curve_crossings_into(curve: &Curve, output: &mut PreparedCurveCrossings) {
    let mut crossings =
        match core::mem::replace(output, PreparedCurveCrossings::Increasing(Vec::new())) {
            PreparedCurveCrossings::Increasing(values)
            | PreparedCurveCrossings::Decreasing(values)
            | PreparedCurveCrossings::Mixed(values) => values,
        };
    crossings.clear();
    let mut increasing = true;
    let mut decreasing = true;
    for pair in curve.points.windows(2) {
        let (start, end) = (&pair[0], &pair[1]);
        increasing &= start.value <= end.value;
        decreasing &= start.value >= end.value;
        let span = end.value - start.value;
        let position_scale = if span.abs() <= 1e-9 {
            0.0
        } else {
            (end.position - start.position) / span
        };
        crossings.push(CrossingSegment {
            position_bias: start.position - start.value * position_scale,
            position_scale,
            min_value: start.value.min(end.value),
            max_value: start.value.max(end.value),
        });
    }
    if let [point] = curve.points.as_slice() {
        crossings.push(CrossingSegment {
            position_bias: point.position,
            position_scale: 0.0,
            min_value: point.value,
            max_value: point.value,
        });
    }
    *output = if increasing {
        PreparedCurveCrossings::Increasing(crossings)
    } else if decreasing {
        PreparedCurveCrossings::Decreasing(crossings)
    } else {
        PreparedCurveCrossings::Mixed(crossings)
    };
}

pub(crate) fn prepared_curve_crossing(
    crossings: &PreparedCurveCrossings,
    raw: &Curve,
    value: f32,
    fallback: f32,
) -> f32 {
    let crossing = |segment: &CrossingSegment| {
        if !(segment.max_value - segment.min_value).is_finite() {
            // Extreme finite endpoints need a wider intermediate difference;
            // their cached f32 slope cannot represent the inverse accurately.
            Some(curve_crossing_raw(raw, value, fallback))
        } else {
            crossing_at(segment, value)
        }
    };
    match crossings.segments() {
        [] => return fallback,
        [segment] => return crossing(segment).unwrap_or(fallback),
        _ => {}
    }
    match crossings {
        PreparedCurveCrossings::Increasing(segments) => {
            let index = segments.partition_point(|segment| segment.max_value < value);
            segments.get(index).and_then(crossing).unwrap_or(fallback)
        }
        PreparedCurveCrossings::Decreasing(segments) => {
            let index = segments.partition_point(|segment| segment.min_value > value);
            segments.get(index).and_then(crossing).unwrap_or(fallback)
        }
        PreparedCurveCrossings::Mixed(segments) => {
            segments.iter().find_map(crossing).unwrap_or(fallback)
        }
    }
}

fn curve_crossing_raw(curve: &Curve, value: f32, fallback: f32) -> f32 {
    crate::sampling::curve_crossing(curve, value, fallback)
}

#[inline(always)]
fn crossing_at(segment: &CrossingSegment, value: f32) -> Option<f32> {
    if !(value >= segment.min_value && value <= segment.max_value) {
        return None;
    }
    Some(segment.position_bias + value * segment.position_scale)
}

impl PreparedCurveCrossings {
    fn segments(&self) -> &[CrossingSegment] {
        match self {
            Self::Increasing(segments) | Self::Decreasing(segments) | Self::Mixed(segments) => {
                segments
            }
        }
    }
}

#[cfg(test)]
mod curve_crossing_tests {
    use super::{Arc, PreparedCurve, prepared_curve_crossing};
    use crate::sampling::curve_crossing;
    use alloc::vec;
    use donder_runtime_types::{Curve, CurvePoint};

    fn prepared(points: &[(f32, f32)]) -> PreparedCurve {
        PreparedCurve::new(Arc::new(Curve {
            points: points
                .iter()
                .map(|&(position, value)| CurvePoint { position, value })
                .collect(),
        }))
    }

    #[test]
    fn prepared_crossing_matches_raw_curves() {
        for points in [
            vec![(0.0, 0.0), (1.0, 1.0)],
            vec![(0.0, 1.0), (0.25, 0.8), (0.6, 0.3), (1.0, 0.0)],
            vec![(0.0, 0.0), (0.3, 1.0), (0.7, 0.2), (1.0, 0.8)],
            vec![(0.0, 0.0), (0.4, 0.0), (0.4, 1.0), (1.0, 1.0)],
        ] {
            let curve = Curve {
                points: points
                    .iter()
                    .map(|&(position, value)| CurvePoint { position, value })
                    .collect(),
            };
            let prepared = prepared(&points);
            for value in [
                f32::NEG_INFINITY,
                -0.1,
                0.0,
                0.1,
                0.2,
                0.5,
                0.8,
                1.0,
                1.1,
                f32::INFINITY,
                f32::NAN,
            ] {
                let expected = curve_crossing(&curve, value, -7.0);
                let actual = prepared_curve_crossing(&prepared.crossings, &curve, value, -7.0);
                assert!(
                    (actual - expected).abs() <= 0.000001,
                    "{points:?} at {value}"
                );
            }
        }
    }

    #[test]
    fn prepared_crossing_preserves_single_point_behavior() {
        let curve = prepared(&[(0.25, 0.75)]);
        assert_eq!(
            prepared_curve_crossing(&curve.crossings, &curve.raw, 0.75, -1.0),
            0.25
        );
        assert_eq!(
            prepared_curve_crossing(&curve.crossings, &curve.raw, 0.5, -1.0),
            -1.0
        );
    }
}

#[cfg(test)]
mod binding_totality_tests {
    use super::{BoundParams, DslBindCache};
    use crate::dsl::{Type, Value};
    use alloc::{sync::Arc, vec};

    #[test]
    fn binding_checks_supplied_values_before_playback() {
        let mut cache = DslBindCache::default();
        let named =
            BoundParams::bind_values(&[Type::Float], vec![Value::Int(3)], &mut cache).unwrap();
        assert!(matches!(named.value(0), Ok(Value::Float(3.0))));
        assert!(
            BoundParams::bind_values(&[Type::Float], vec![Value::Bool(true)], &mut cache).is_err()
        );
        assert!(BoundParams::bind_values(&[Type::Float], vec![], &mut cache).is_err());
        assert!(
            BoundParams::bind_values(
                &[Type::array(Type::Int)],
                vec![Value::Array(Arc::from(vec![Value::Bool(true)]))],
                &mut cache
            )
            .is_err()
        );
    }
}
