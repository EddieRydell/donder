mod arrays;
mod context;
pub(crate) use context::NoSignals;
use context::{Clock, ReadContext};
mod automation;
mod batch;
pub(crate) use batch::{
    Batch, BatchSignals, BatchWorkspace, LANES as BATCH_LANES, Lanes, Mask as BatchMask,
};
mod parameters;
pub(crate) use automation::AutomationPlan;

use parameters::{
    CurveRegister, GradientRegister, MarksRegister, ParameterAddress, ParameterValues,
};

use super::bytecode::{
    BytecodeProgram, ColorBinary, ColorComponent, ColorSlot, CompareOp, ContextRead, FloatBinary,
    FloatUnary, Instruction, MarkOp, NumberSlot, ParameterKind, SignalPixel, SlotLayout, ValueSlot,
};
use super::types::{Identifier, Type, Value};
use crate::sampling::{
    clamp_float, color_hue, color_intensity, color_saturation, invert_color, scale_color,
};
use crate::values::{Color, Curve, Gradient, Marks, SampleDuration};
use alloc::boxed::Box;
#[cfg(test)]
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use donder_language::Shared as Arc;

use donder_language::execution::SpatialContext;

#[derive(Clone, Copy, Debug)]
pub(crate) struct RunContext {
    pub progress: f32,
    pub time: SampleDuration,
    pub duration: SampleDuration,
    pub pixel_index: i32,
    pub pixel_count: i32,
    pub pixel_fraction: f32,
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
        params: &donder_language::dsl::BoundParams,
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
        let accepted = donder_language::dsl::BoundParams::bind_values(types, values)
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
        Ok(runtime_to_value(
            value.to_runtime(),
            &ArrayStorage::default(),
        ))
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
                            size_of::<crate::values::CurvePoint>() + size_of::<CrossingSegment>(),
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
    fn to_runtime(&self) -> RuntimeValue {
        match self {
            Self::Void => RuntimeValue::Void,
            Self::Int(value) => RuntimeValue::Int(*value),
            Self::Float(value) => RuntimeValue::Float(*value),
            Self::Bool(value) => RuntimeValue::Bool(*value),
            Self::Color(value) => RuntimeValue::Color(*value),
            Self::Marks(value) => RuntimeValue::Marks(Arc::clone(value)),
            Self::Curve(value) => RuntimeValue::PreparedCurve(Arc::clone(value)),
            Self::RawCurve(value) => RuntimeValue::Curve(Arc::clone(value)),
            Self::Gradient(value) => RuntimeValue::Gradient(Arc::clone(value)),
            Self::Array(value) => RuntimeValue::Array(Arc::clone(value)),
            Self::Enum(value) => RuntimeValue::Enum(value.clone()),
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
        crate::automation::curve_window_into(
            Arc::make_mut(&mut self.raw),
            curve,
            min,
            max,
            position,
        );
        prepare_curve_crossings_into(&self.raw, Arc::make_mut(&mut self.crossings));
    }
}

// Slots have a compiler-bounded width, so allocation cannot fragment the value
// buffer. Counts represent register roots and array children, not temporary
// borrowed handles returned by value()/index_value(). No atomics or GC pass.
#[derive(Clone, Default)]
struct ArrayStorage {
    free: Vec<usize>,
    references: Vec<usize>,
    lengths: Vec<usize>,
    values: Vec<RuntimeValue>,
    width: usize,
}

impl core::fmt::Debug for ArrayStorage {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ArrayStorage")
            .field("capacity", &self.references.len())
            .field("width", &self.width)
            .finish()
    }
}

impl ArrayStorage {
    fn new(capacity: usize, width: usize) -> Self {
        Self {
            free: (0..capacity).rev().collect(),
            references: vec![0; capacity],
            lengths: vec![0; capacity],
            values: vec![RuntimeValue::Void; capacity * width],
            width,
        }
    }

    /// Take one reserved construction slot. Bytecode admission bounds the width
    /// of every MakeArray and all live array nodes, plus this extra slot.
    /// Copying into caller-sized result buffers checks its dimensions separately.
    fn allocate(&mut self, len: usize) -> usize {
        let remaining = self.free.len() - 1;
        let index = self.free[remaining];
        self.free.truncate(remaining);
        self.references[index] = 1; // Construction root; transferred by set_array.
        self.lengths[index] = len;
        index
    }

    fn items(&self, index: usize) -> &[RuntimeValue] {
        let start = index * self.width;
        &self.values[start..start + self.lengths[index]]
    }

    fn retain(&mut self, value: &RuntimeValue) {
        if let RuntimeValue::ArraySlot(index) = value {
            self.references[*index] += 1;
        }
    }

    fn release(&mut self, value: RuntimeValue) {
        let RuntimeValue::ArraySlot(index) = value else {
            return;
        };
        self.references[index] -= 1;
        if self.references[index] != 0 {
            return;
        }
        let start = index * self.width;
        for offset in start..start + self.lengths[index] {
            let child = core::mem::replace(&mut self.values[offset], RuntimeValue::Void);
            self.release(child);
        }
        self.lengths[index] = 0;
        self.free.push(index);
    }
}

#[derive(Clone, Debug)]
enum RuntimeValue {
    Void,
    Int(i32),
    Float(f32),
    Bool(bool),
    Color(Color),
    Marks(Arc<Marks>),
    Curve(Arc<Curve>),
    Gradient(Arc<Gradient>),
    PreparedCurve(Arc<PreparedCurve>),
    Array(Arc<[Value]>),
    ArraySlot(usize),
    Enum(Identifier),
}

impl RuntimeValue {
    fn from_value(value: &Value) -> Self {
        match value {
            Value::Void => Self::Void,
            Value::Int(value) => Self::Int(*value),
            Value::Float(value) => Self::Float(*value),
            Value::Bool(value) => Self::Bool(*value),
            Value::Color(value) => Self::Color(*value),
            Value::Marks(value) => Self::Marks(Arc::clone(value)),
            Value::Curve(value) => Self::Curve(Arc::clone(value)),
            Value::Gradient(value) => Self::Gradient(Arc::clone(value)),
            Value::Array(value) => Self::Array(Arc::clone(value)),
            Value::Enum(value) => Self::Enum(value.clone()),
        }
    }
}

fn clone_runtime(value: &RuntimeValue) -> RuntimeValue {
    match value {
        RuntimeValue::Void => RuntimeValue::Void,
        RuntimeValue::Int(value) => RuntimeValue::Int(*value),
        RuntimeValue::Float(value) => RuntimeValue::Float(*value),
        RuntimeValue::Bool(value) => RuntimeValue::Bool(*value),
        RuntimeValue::Color(value) => RuntimeValue::Color(*value),
        RuntimeValue::Marks(value) => RuntimeValue::Marks(Arc::clone(value)),
        RuntimeValue::Curve(value) => RuntimeValue::Curve(Arc::clone(value)),
        RuntimeValue::Gradient(value) => RuntimeValue::Gradient(Arc::clone(value)),
        RuntimeValue::PreparedCurve(value) => RuntimeValue::PreparedCurve(Arc::clone(value)),
        RuntimeValue::Array(value) => RuntimeValue::Array(Arc::clone(value)),
        RuntimeValue::ArraySlot(index) => RuntimeValue::ArraySlot(*index),
        RuntimeValue::Enum(value) => RuntimeValue::Enum(value.clone()),
    }
}

fn clamp_array_index(index: i32, nonempty_length: usize) -> usize {
    (index.max(0) as usize).min(nonempty_length - 1)
}

fn int_len(length: usize) -> i32 {
    i32::try_from(length).unwrap_or(i32::MAX)
}

#[cfg(test)]
fn runtime_to_value(value: RuntimeValue, arrays: &ArrayStorage) -> Value {
    match value {
        RuntimeValue::Void => Value::Void,
        RuntimeValue::Int(value) => Value::Int(value),
        RuntimeValue::Float(value) => Value::Float(value),
        RuntimeValue::Bool(value) => Value::Bool(value),
        RuntimeValue::Color(value) => Value::Color(value),
        RuntimeValue::Marks(value) => Value::Marks(value),
        RuntimeValue::Curve(value) => Value::Curve(value),
        RuntimeValue::PreparedCurve(value) => Value::Curve(value.raw()),
        RuntimeValue::Gradient(value) => Value::Gradient(value),
        RuntimeValue::Array(value) => Value::Array(value),
        RuntimeValue::ArraySlot(index) => Value::Array(
            arrays
                .items(index)
                .iter()
                .map(|value| runtime_to_value(clone_runtime(value), arrays))
                .collect::<Vec<_>>()
                .into(),
        ),
        RuntimeValue::Enum(value) => Value::Enum(value),
    }
}

fn black() -> Color {
    Color {
        red: 0,
        green: 0,
        blue: 0,
    }
}

fn runtime_refs_equal(left: &RuntimeValue, right: &RuntimeValue) -> bool {
    match (left, right) {
        (RuntimeValue::Void, RuntimeValue::Void) => true,
        (RuntimeValue::Int(left), RuntimeValue::Int(right)) => left == right,
        (RuntimeValue::Float(left), RuntimeValue::Float(right)) => left == right,
        (RuntimeValue::Int(left), RuntimeValue::Float(right)) => (*left as f32) == *right,
        (RuntimeValue::Float(left), RuntimeValue::Int(right)) => *left == (*right as f32),
        (RuntimeValue::Bool(left), RuntimeValue::Bool(right)) => left == right,
        (RuntimeValue::Color(left), RuntimeValue::Color(right)) => left == right,
        (RuntimeValue::Enum(left), RuntimeValue::Enum(right)) => left == right,
        _ => false,
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
    use crate::values::{Curve, CurvePoint};
    use alloc::vec;

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

fn sample_curve(curve: &Curve, position: f32) -> f32 {
    crate::sampling::sample_curve(curve, position)
}

fn sample_gradient(gradient: &Gradient, position: f32) -> Color {
    crate::sampling::sample_gradient(gradient, position)
}

/// Unary math propagates NaN before its implementation sees it.
#[inline(always)]
fn float_unary(op: FloatUnary, value: f32) -> f32 {
    if value.is_nan() {
        return f32::NAN;
    }
    match op {
        FloatUnary::Sin => micromath::F32Ext::sin(value),
        FloatUnary::Cos => micromath::F32Ext::cos(value),
        FloatUnary::Abs => value.abs(),
        FloatUnary::Floor => libm::floorf(value),
        FloatUnary::Ceil => libm::ceilf(value),
        FloatUnary::Trunc => libm::truncf(value),
        FloatUnary::Sqrt => libm::sqrtf(value),
    }
}

#[inline(always)]
fn smoothstep(value: f32) -> f32 {
    let t = value.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A source query inside the effect duration, in seconds; NaN outside it.
fn query_seconds(seconds: f32, duration: SampleDuration) -> f32 {
    crate::values::sample_time_from_seconds_f32(seconds)
        .ok()
        .filter(|time| time.as_ticks() < duration.as_ticks())
        .map_or(f32::NAN, crate::values::sample_time_seconds_f32)
}

fn query_progress(seconds: f32, duration: SampleDuration) -> f32 {
    let duration = duration.as_ticks();
    crate::values::sample_time_from_seconds_f32(seconds)
        .ok()
        .filter(|time| time.as_ticks() < duration)
        .map_or(f32::NAN, |time| {
            (time.as_ticks() as f32 / duration as f32).clamp(0.0, 1.0)
        })
}

#[inline(always)]
fn section_position(pixel_index: i32, width: f32, inverse: f32) -> f32 {
    let index = pixel_index as f32;
    (index - libm::floorf(index * inverse) * width) * inverse
}

#[inline(always)]
fn gradient_color_scaled(gradient: &Gradient, position: f32, scale: f32) -> Color {
    let scale = scale.clamp(0.0, 1.0);
    if scale <= 0.0 {
        black()
    } else {
        scale_color(sample_gradient(gradient, position), scale)
    }
}

fn float_binary(op: FloatBinary, left: f32, right: f32) -> f32 {
    match op {
        FloatBinary::ValueOr => {
            if left.is_nan() {
                right
            } else {
                left
            }
        }
        FloatBinary::Min | FloatBinary::Max if left.is_nan() || right.is_nan() => f32::NAN,
        FloatBinary::Min => left.min(right),
        FloatBinary::Max => left.max(right),
        FloatBinary::Atan2 => libm::atan2f(left, right),
    }
}

fn mark_at_from(marks: &Marks, index: i32) -> f32 {
    usize::try_from(index)
        .ok()
        .and_then(|index| marks.seconds().get(index))
        .copied()
        .unwrap_or(f32::NAN)
}

/// Marks are chronological and seconds conversion is monotonic, so the marks
/// at or before `seconds` form a prefix. A NaN query matches no mark.
/// Returns the mark's index and its time in seconds.
fn previous_mark(marks: &Marks, seconds: f32) -> Option<(usize, f32)> {
    let times = marks.seconds();
    let index = times
        .partition_point(|&mark| mark <= seconds)
        .checked_sub(1)?;
    Some((index, times[index]))
}

fn prev_index(marks: &Marks, seconds: f32) -> i32 {
    previous_mark(marks, seconds)
        .map(|(index, _)| int_len(index))
        .unwrap_or(-1)
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
