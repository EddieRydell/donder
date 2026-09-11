use super::GeneratedEffectSlot;
use super::bytecode::{
    ArithmeticOp, BoolSlot, BytecodeProgram, ColorBinary, ColorSlot, CompareOp, ContextRead,
    FloatBinary, FloatSlot, FloatUnary, GeneratorContextId, Instruction, IntArithmeticOp, IntSlot,
    MarkOp, RefSlot, SignalPixel, SlotLayout, TargetItemsOp, ValueSlot,
};
use super::types::{Identifier, Type, Value};
use super::types::{TargetItemValue, TargetItemsValue, TargetPixelValue, TargetValue};
use super::{CompiledEffect, CompiledOperator, EffectKind, ParamDecl};
use crate::automation::{AutomationMapping, AutomationValue, automation_value_at_position};
use crate::sampling::{
    add_colors, color_intensity, invert_color, max_colors, mix_colors, multiply_colors, scale_color,
};
use crate::values::{
    Color, Curve, Gradient, Marks, SampleDuration, SampleTime, SampleTimeError,
    sample_duration_from_seconds_f32, sample_duration_seconds_f32, sample_time_with_seconds_offset,
};
#[cfg(not(feature = "atomic"))]
use alloc::rc::Rc as Arc;
use alloc::string::String;
#[cfg(feature = "atomic")]
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use alloc::{boxed::Box, format};

pub const MAX_VM_INSTRUCTIONS_PER_INVOCATION: usize = 10_000;

#[derive(Clone, Debug)]
pub struct RunContext {
    pub progress: f32,
    pub time: SampleDuration,
    pub duration: SampleDuration,
    pub pixel_index: i32,
    pub pixel_count: i32,
    pub pixel_fraction: f32,
}

pub type OperatorRunContext = RunContext;

/// Samples an immutable signal. Identical input/time/pixel
/// queries must produce the same result; compilation and evaluation may reuse it.
pub trait SignalSampler {
    fn sample_signal(
        &mut self,
        input: usize,
        sample_time: SampleTime,
        pixel: SignalPixel<i32>,
        frame_cache: Option<usize>,
    ) -> Result<Color, RuntimeError>;
}

#[derive(Clone, Debug)]
pub struct GeneratorContext {
    pub start_time: SampleTime,
    pub duration: SampleDuration,
    pub target: Arc<TargetValue>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeneratedEffect {
    pub definition: GeneratedEffectSlot,
    pub start_time: SampleTime,
    pub duration: SampleDuration,
    pub target: Arc<TargetItemValue>,
    pub params: Vec<(Identifier, Value)>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeError {
    pub message: String,
}

impl RuntimeError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

#[derive(Clone, Debug, Default, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct BoundParams {
    values: Vec<BoundParamValue>,
    /// Playback-only storage for calculated arrays. Frozen parameters never
    /// contain handles into this arena.
    #[rkyv(with = rkyv::with::Skip)]
    arrays: Option<Box<ArrayStorage>>,
}

impl BoundParams {
    pub(crate) fn result_storage_estimate(
        count: usize,
        capacity: u32,
        width: u32,
    ) -> Option<usize> {
        count
            .checked_mul(size_of::<BoundParamValue>())?
            .checked_add(if capacity == 0 {
                0
            } else {
                size_of::<ArrayStorage>()
                    .checked_add((capacity as usize).checked_mul(3 * size_of::<u32>())?)?
                    .checked_add(
                        (capacity as usize)
                            .checked_mul(width as usize)?
                            .checked_mul(size_of::<RuntimeValue>())?,
                    )?
            })
    }
    /// Allocate typed-result storage during workspace creation.
    pub fn result_workspace(count: usize, array_capacity: u32, array_width: u32) -> Self {
        Self {
            values: vec![BoundParamValue::Void; count],
            arrays: (array_capacity != 0)
                .then(|| Box::new(ArrayStorage::new(array_capacity, array_width))),
        }
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Materialize an owned value during host preparation or inspection.
    pub fn value(&self, index: usize) -> Result<Value, RuntimeError> {
        let value = self
            .values
            .get(index)
            .ok_or_else(|| RuntimeError::new("invalid parameter slot"))?;
        Ok(runtime_to_value(
            value.to_runtime(),
            None,
            self.arrays.as_deref(),
        ))
    }

    /// Release forwarded resources before changing an ancestor's automation.
    /// The arena and parameter slots remain allocated.
    pub fn clear_results(&mut self) {
        for index in 0..self.values.len() {
            self.clear_parameter(index);
        }
    }

    pub(crate) fn clear_parameter(&mut self, index: usize) {
        if let BoundParamValue::CalculatedArray(index) =
            core::mem::replace(&mut self.values[index], BoundParamValue::Void)
        {
            self.arrays
                .as_mut()
                .expect("prepared result arena")
                .release(RuntimeValue::ArraySlot(index));
        }
    }

    pub(crate) fn reserve_result_arrays(&mut self, capacity: u32, width: u32) {
        if capacity != 0 {
            self.arrays = Some(Box::new(ArrayStorage::new(capacity, width)));
        }
    }

    pub fn bind_slots(
        types: &[Type],
        values: &[Option<Value>],
        cache: &mut DslBindCache,
    ) -> Result<Self, RuntimeError> {
        if types.len() != values.len() {
            return Err(RuntimeError::new(
                "parameter slot count does not match its types",
            ));
        }
        Ok(Self {
            values: types
                .iter()
                .zip(values)
                .map(|(ty, value)| {
                    value.as_ref().map_or(BoundParamValue::Void, |value| {
                        bind_param_value(ty, value.clone(), cache)
                    })
                })
                .collect(),
            arrays: None,
        })
    }

    pub(crate) fn is_frozen(&self) -> bool {
        self.arrays.is_none()
            && self
                .values
                .iter()
                .all(|value| !matches!(value, BoundParamValue::CalculatedArray(_)))
    }

    /// Forward a typed slot, copying calculated array storage into the prepared
    /// destination arena while retaining resource identities.
    pub fn copy_parameter(
        &mut self,
        destination: usize,
        source: &Self,
        index: usize,
        ty: &Type,
    ) -> Result<(), RuntimeError> {
        let value = source
            .values
            .get(index)
            .ok_or_else(|| RuntimeError::new("invalid parameter binding source"))?
            .to_runtime();
        self.write_result(destination, value, ty, None, source.arrays.as_deref())
    }

    fn write_result(
        &mut self,
        index: usize,
        value: RuntimeValue,
        ty: &Type,
        arrays: Option<&ArrayStorage>,
        parameters: Option<&ArrayStorage>,
    ) -> Result<(), RuntimeError> {
        let output = self
            .values
            .get_mut(index)
            .ok_or_else(|| RuntimeError::new("invalid parameter binding destination"))?;
        if let BoundParamValue::CalculatedArray(index) =
            core::mem::replace(output, BoundParamValue::Void)
        {
            self.arrays
                .as_mut()
                .expect("prepared result arena")
                .release(RuntimeValue::ArraySlot(index));
        }
        *output = match (ty, value) {
            (Type::Float, RuntimeValue::Int(value)) => BoundParamValue::Float(value as f32),
            (_, RuntimeValue::Void) => BoundParamValue::Void,
            (_, RuntimeValue::Int(value)) => BoundParamValue::Int(value),
            (_, RuntimeValue::Float(value)) => BoundParamValue::Float(value),
            (_, RuntimeValue::Bool(value)) => BoundParamValue::Bool(value),
            (_, RuntimeValue::Color(value)) => BoundParamValue::Color(value),
            (_, RuntimeValue::Marks(value)) => BoundParamValue::Marks(value),
            (_, RuntimeValue::Target(value)) => BoundParamValue::Target(value),
            (_, RuntimeValue::TargetItems(value)) => BoundParamValue::TargetItems(value),
            (_, RuntimeValue::TargetItem(value)) => BoundParamValue::TargetItem(value),
            (_, RuntimeValue::Curve(value)) => BoundParamValue::RawCurve(value),
            (_, RuntimeValue::PreparedCurve(value)) => BoundParamValue::Curve(value),
            (_, RuntimeValue::Gradient(value)) => BoundParamValue::Gradient(value),
            (_, RuntimeValue::Array(value)) => BoundParamValue::Array(value),
            (_, value @ (RuntimeValue::ArraySlot(_) | RuntimeValue::ParameterArray(_))) => {
                let arena = self
                    .arrays
                    .as_mut()
                    .ok_or_else(|| RuntimeError::new("missing result array storage"))?;
                let RuntimeValue::ArraySlot(index) =
                    arena.copy_array(&value, arrays, parameters)?
                else {
                    unreachable!("copy_array returns an owned array slot")
                };
                BoundParamValue::CalculatedArray(index)
            }
            (_, RuntimeValue::Enum(value)) => BoundParamValue::Enum(value),
            (_, RuntimeValue::Timeline) => {
                return Err(RuntimeError::new("timeline cannot be a parameter"));
            }
        };
        Ok(())
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
            .checked_mul(size_of::<BoundParamValue>())?;
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

    pub(crate) fn clone_for_automation(&self) -> Self {
        Self {
            values: self
                .values
                .iter()
                .map(|value| match value {
                    BoundParamValue::Curve(curve) => {
                        BoundParamValue::Curve(Arc::new(curve.detached_clone()))
                    }
                    value => value.clone(),
                })
                .collect(),
            arrays: self.arrays.clone(),
        }
    }

    pub(crate) fn reserve_automation(
        &mut self,
        param_index: usize,
        curve: &Curve,
        mapping: &AutomationMapping,
    ) {
        let Some(value) = self.values.get_mut(param_index) else {
            return;
        };
        if let (BoundParamValue::Curve(value), AutomationMapping::Curve { .. }) = (value, mapping) {
            Arc::make_mut(value).reserve_window_capacity(curve.points.len());
        }
    }

    pub fn bind<'a, P>(declarations: &[ParamDecl], params: P) -> Result<Self, RuntimeError>
    where
        P: Clone + IntoIterator<Item = (&'a Identifier, &'a Value)>,
    {
        Self::bind_cached(declarations, params, &mut DslBindCache::default())
    }

    pub fn bind_cached<'a, P>(
        declarations: &[ParamDecl],
        params: P,
        cache: &mut DslBindCache,
    ) -> Result<Self, RuntimeError>
    where
        P: Clone + IntoIterator<Item = (&'a Identifier, &'a Value)>,
    {
        if let Some(name) = params
            .clone()
            .into_iter()
            .map(|(name, _)| name)
            .find(|name| !declarations.iter().any(|param| param.name == **name))
        {
            return Err(RuntimeError::new(format!(
                "unknown parameter `{}`",
                name.as_str()
            )));
        }
        let mut bound = Self::default();
        bind_values(declarations, cache, &mut bound, |param| {
            resolve_param(param, params.clone())
        })?;
        Ok(bound)
    }

    pub fn bind_pairs(
        declarations: &[ParamDecl],
        params: &[(Identifier, Value)],
    ) -> Result<Self, RuntimeError> {
        Self::bind(
            declarations,
            params.iter().map(|(name, value)| (name, value)),
        )
    }

    pub fn bind_pairs_cached(
        declarations: &[ParamDecl],
        params: &[(Identifier, Value)],
        cache: &mut DslBindCache,
    ) -> Result<Self, RuntimeError> {
        Self::bind_cached(
            declarations,
            params.iter().map(|(name, value)| (name, value)),
            cache,
        )
    }

    pub fn apply_automation(
        &mut self,
        param_index: usize,
        automation_curve: &Curve,
        mapping: &AutomationMapping,
        position: f32,
    ) -> Result<(), RuntimeError> {
        let value = self
            .values
            .get_mut(param_index)
            .ok_or_else(|| RuntimeError::new("invalid automated parameter slot"))?;
        if let AutomationMapping::Curve { min, max } = mapping {
            let BoundParamValue::Curve(curve) = value else {
                return Err(RuntimeError::new(
                    "curve automation targets a non-curve parameter",
                ));
            };
            Arc::make_mut(curve).update_window(automation_curve, *min, *max, position);
            return Ok(());
        }
        let automated = automation_value_at_position(automation_curve, mapping, position)
            .ok_or_else(|| RuntimeError::new("enum automation mapping has no values"))?;
        value.update_automation(automated)
    }

    pub fn int(&self, index: usize) -> Result<i32, RuntimeError> {
        match self.values.get(index) {
            Some(BoundParamValue::Int(value)) => Ok(*value),
            _ => Err(RuntimeError::new("expected int parameter")),
        }
    }

    pub fn float(&self, index: usize) -> Result<f32, RuntimeError> {
        match self.values.get(index) {
            Some(BoundParamValue::Float(value)) => Ok(*value),
            Some(BoundParamValue::Int(value)) => Ok(*value as f32),
            _ => Err(RuntimeError::new("expected float parameter")),
        }
    }

    pub fn boolean(&self, index: usize) -> Result<bool, RuntimeError> {
        match self.values.get(index) {
            Some(BoundParamValue::Bool(value)) => Ok(*value),
            _ => Err(RuntimeError::new("expected bool parameter")),
        }
    }

    pub fn color(&self, index: usize) -> Result<Color, RuntimeError> {
        match self.values.get(index) {
            Some(BoundParamValue::Color(value)) => Ok(*value),
            _ => Err(RuntimeError::new("expected color parameter")),
        }
    }

    pub fn marks(&self, index: usize) -> Result<Arc<Marks>, RuntimeError> {
        match self.values.get(index) {
            Some(BoundParamValue::Marks(value)) => Ok(Arc::clone(value)),
            _ => Err(RuntimeError::new("expected marks parameter")),
        }
    }

    pub fn curve(&self, index: usize) -> Result<Arc<Curve>, RuntimeError> {
        match self.values.get(index) {
            Some(BoundParamValue::Curve(value)) => Ok(value.raw()),
            Some(BoundParamValue::RawCurve(value)) => Ok(Arc::clone(value)),
            _ => Err(RuntimeError::new("expected curve parameter")),
        }
    }

    pub(crate) fn prepared_curve_crossings(
        &self,
        index: usize,
    ) -> Result<CurveCrossings, RuntimeError> {
        match self.values.get(index) {
            Some(BoundParamValue::Curve(value)) => {
                Ok(CurveCrossings::Prepared(Arc::clone(&value.crossings)))
            }
            Some(BoundParamValue::RawCurve(value)) => Ok(CurveCrossings::Raw(Arc::clone(value))),
            _ => Err(RuntimeError::new("expected curve parameter")),
        }
    }

    pub fn gradient(&self, index: usize) -> Result<Arc<Gradient>, RuntimeError> {
        match self.values.get(index) {
            Some(BoundParamValue::Gradient(value)) => Ok(Arc::clone(value)),
            _ => Err(RuntimeError::new("expected gradient parameter")),
        }
    }

    pub fn array(&self, index: usize) -> Result<&[Value], RuntimeError> {
        match self.values.get(index) {
            Some(BoundParamValue::Array(value)) => Ok(value),
            _ => Err(RuntimeError::new("expected array parameter")),
        }
    }

    pub fn array_len(&self, index: usize) -> Result<usize, RuntimeError> {
        let value = self
            .values
            .get(index)
            .ok_or_else(|| RuntimeError::new("invalid array parameter"))?
            .to_runtime();
        array_length(&value, None, self.arrays.as_deref())
    }

    pub fn gradient_at(
        &self,
        parameter: usize,
        index: usize,
    ) -> Result<Arc<Gradient>, RuntimeError> {
        match self.array_value(parameter, index)? {
            RuntimeValue::Gradient(value) => Ok(value),
            _ => Err(RuntimeError::new("expected gradient array fixture")),
        }
    }

    pub fn curve_at(&self, parameter: usize, index: usize) -> Result<Arc<Curve>, RuntimeError> {
        match self.array_value(parameter, index)? {
            RuntimeValue::Curve(value) => Ok(value),
            RuntimeValue::PreparedCurve(value) => Ok(value.raw()),
            _ => Err(RuntimeError::new("expected curve array fixture")),
        }
    }

    fn array_value(&self, parameter: usize, index: usize) -> Result<RuntimeValue, RuntimeError> {
        let value = self
            .values
            .get(parameter)
            .ok_or_else(|| RuntimeError::new("invalid array parameter"))?
            .to_runtime();
        array_item(&value, index, None, self.arrays.as_deref())
    }

    pub fn enum_name(&self, index: usize) -> Result<&str, RuntimeError> {
        match self.values.get(index) {
            Some(BoundParamValue::Enum(value)) => Ok(value.as_str()),
            _ => Err(RuntimeError::new("expected enum parameter")),
        }
    }

    pub fn sample_curve(&self, index: usize, position: f32) -> Result<f32, RuntimeError> {
        match self.values.get(index) {
            Some(BoundParamValue::Curve(value)) => sample_prepared_curve(value, position),
            Some(BoundParamValue::RawCurve(value)) => Ok(sample_curve(value, position)),
            _ => Err(RuntimeError::new("expected curve parameter")),
        }
    }

    pub fn curve_crossing(
        &self,
        index: usize,
        value: f32,
        fallback: f32,
    ) -> Result<f32, RuntimeError> {
        let curve = match self.values.get(index) {
            Some(BoundParamValue::Curve(value)) => value,
            Some(BoundParamValue::RawCurve(curve)) => {
                return Ok(curve_crossing_raw(curve, value, fallback));
            }
            _ => return Err(RuntimeError::new("expected curve parameter")),
        };
        prepared_curve_crossing(&curve.crossings, value, fallback)
    }

    pub fn sample_gradient(&self, index: usize, position: f32) -> Result<Color, RuntimeError> {
        match self.values.get(index) {
            Some(BoundParamValue::Gradient(value)) => sample_gradient(value, position),
            _ => Err(RuntimeError::new("expected gradient parameter")),
        }
    }
}

#[derive(Debug, Default)]
pub struct DslBindCache {
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
    Target(Arc<TargetValue>),
    TargetItems(Arc<TargetItemsValue>),
    TargetItem(Arc<TargetItemValue>),
    Curve(Arc<PreparedCurve>),
    RawCurve(Arc<Curve>),
    Gradient(Arc<Gradient>),
    Array(Arc<[Value]>),
    CalculatedArray(u32),
    Enum(Identifier),
}

impl BoundParamValue {
    fn from_value(_ty: &Type, value: Value, cache: &mut DslBindCache) -> Self {
        match value {
            Value::Void => Self::Void,
            Value::Int(value) => Self::Int(value),
            Value::Float(value) => Self::Float(value),
            Value::Bool(value) => Self::Bool(value),
            Value::Color(value) => Self::Color(value),
            Value::Marks(value) => Self::Marks(value),
            Value::Target(value) => Self::Target(value),
            Value::TargetItems(value) => Self::TargetItems(value),
            Value::TargetItem(value) => Self::TargetItem(value),
            Value::Curve(value) => Self::Curve(cache.prepared_curve(value)),
            Value::Gradient(value) => Self::Gradient(value),
            Value::Array(value) => Self::Array(value),
            Value::Enum(value) => Self::Enum(value),
        }
    }

    fn to_runtime(&self) -> RuntimeValue {
        match self {
            Self::Void => RuntimeValue::Void,
            Self::Int(value) => RuntimeValue::Int(*value),
            Self::Float(value) => RuntimeValue::Float(*value),
            Self::Bool(value) => RuntimeValue::Bool(*value),
            Self::Color(value) => RuntimeValue::Color(*value),
            Self::Marks(value) => RuntimeValue::Marks(Arc::clone(value)),
            Self::Target(value) => RuntimeValue::Target(Arc::clone(value)),
            Self::TargetItems(value) => RuntimeValue::TargetItems(Arc::clone(value)),
            Self::TargetItem(value) => RuntimeValue::TargetItem(Arc::clone(value)),
            Self::Curve(value) => RuntimeValue::PreparedCurve(Arc::clone(value)),
            Self::RawCurve(value) => RuntimeValue::Curve(Arc::clone(value)),
            Self::Gradient(value) => RuntimeValue::Gradient(Arc::clone(value)),
            Self::Array(value) => RuntimeValue::Array(Arc::clone(value)),
            Self::CalculatedArray(index) => RuntimeValue::ParameterArray(*index),
            Self::Enum(value) => RuntimeValue::Enum(value.clone()),
        }
    }

    fn update_automation(&mut self, value: AutomationValue<'_>) -> Result<(), RuntimeError> {
        match (self, value) {
            (Self::Int(output), AutomationValue::Int(value)) => *output = value,
            (Self::Float(output), AutomationValue::Float(value)) => *output = value,
            (Self::Bool(output), AutomationValue::Bool(value)) => *output = value,
            (Self::Enum(output), AutomationValue::Enum(value)) => output.clone_from(value),
            _ => {
                return Err(RuntimeError::new(
                    "automation value has the wrong parameter type",
                ));
            }
        }
        Ok(())
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

#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) enum CurveCrossings {
    Prepared(Arc<PreparedCurveCrossings>),
    Raw(Arc<Curve>),
}

impl CurveCrossings {
    pub(crate) fn crossing(&self, value: f32, fallback: f32) -> Result<f32, RuntimeError> {
        match self {
            Self::Prepared(curve) => prepared_curve_crossing(curve, value, fallback),
            Self::Raw(curve) => Ok(curve_crossing_raw(curve, value, fallback)),
        }
    }
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

#[derive(Debug, Default)]
pub struct VmWorkspace {
    registers: VmRegisters,
    arrays: Option<Box<ArrayStorage>>,
}

impl VmWorkspace {
    pub fn for_program(program: &BytecodeProgram) -> Self {
        let mut workspace = Self::default();
        workspace.reserve(program);
        workspace
    }

    pub(crate) fn storage_estimate(
        registers: [usize; 5],
        capacity: usize,
        width: usize,
    ) -> Option<usize> {
        let sizes = [
            size_of::<i32>(),
            size_of::<f32>(),
            size_of::<bool>(),
            size_of::<Color>(),
            size_of::<RuntimeValue>(),
        ];
        let mut bytes = size_of::<Self>();
        for (count, size) in registers.into_iter().zip(sizes) {
            bytes = bytes.checked_add(count.checked_mul(size)?)?;
        }
        if capacity != 0 {
            bytes = bytes
                .checked_add(size_of::<ArrayStorage>())?
                .checked_add(capacity.checked_mul(3 * size_of::<u32>())?)?
                .checked_add(
                    capacity
                        .checked_mul(width)?
                        .checked_mul(size_of::<RuntimeValue>())?,
                )?;
        }
        Some(bytes)
    }

    pub fn reserve(&mut self, bytecode: &BytecodeProgram) {
        self.registers.reserve(bytecode.layout);
        self.reserve_arrays(bytecode);
    }

    fn reserve_arrays(&mut self, bytecode: &BytecodeProgram) {
        if bytecode.array_capacity == 0 {
            return;
        }
        let (capacity, width) = self.arrays.as_ref().map_or((0, 0), |arrays| {
            (arrays.references.len() as u32, arrays.width)
        });
        if capacity < bytecode.array_capacity || width < bytecode.array_width {
            self.arrays = Some(Box::new(ArrayStorage::new(
                capacity.max(bytecode.array_capacity),
                width.max(bytecode.array_width),
            )));
        }
    }
}

// Slots have a compiler-bounded width, so allocation cannot fragment the value
// buffer. Counts represent register roots and array children, not temporary
// borrowed handles returned by value()/index_value(). No atomics or GC pass.
#[derive(Clone)]
struct ArrayStorage {
    free: Vec<u32>,
    references: Vec<u32>,
    lengths: Vec<u32>,
    values: Vec<RuntimeValue>,
    width: u32,
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
    fn copy_array(
        &mut self,
        value: &RuntimeValue,
        arrays: Option<&Self>,
        parameters: Option<&Self>,
    ) -> Result<RuntimeValue, RuntimeError> {
        let length = array_length(value, arrays, parameters)?;
        let slot = self.allocate(length)?;
        let result = (|| {
            for index in 0..length {
                let value = array_item(value, index, arrays, parameters)?;
                let value = match value {
                    RuntimeValue::ArraySlot(_) | RuntimeValue::ParameterArray(_) => {
                        self.copy_array(&value, arrays, parameters)?
                    }
                    value => value,
                };
                self.values[slot as usize * self.width as usize + index] = value;
            }
            Ok(RuntimeValue::ArraySlot(slot))
        })();
        if result.is_err() {
            self.release(RuntimeValue::ArraySlot(slot));
        }
        result
    }

    fn new(capacity: u32, width: u32) -> Self {
        Self {
            free: (0..capacity).rev().collect(),
            references: vec![0; capacity as usize],
            lengths: vec![0; capacity as usize],
            values: vec![RuntimeValue::Void; capacity as usize * width as usize],
            width,
        }
    }

    fn allocate(&mut self, len: usize) -> Result<u32, RuntimeError> {
        if len > self.width as usize {
            return Err(RuntimeError::new("array exceeds prepared width"));
        }
        let slot = self
            .free
            .pop()
            .ok_or_else(|| RuntimeError::new("array storage exhausted"))?;
        let index = slot as usize;
        self.references[index] = 1; // Construction root; transferred by set_ref.
        self.lengths[index] = len as u32;
        Ok(index as u32)
    }

    fn items(&self, index: u32) -> &[RuntimeValue] {
        let start = index as usize * self.width as usize;
        &self.values[start..start + self.lengths[index as usize] as usize]
    }

    fn retain(&mut self, value: &RuntimeValue) {
        if let RuntimeValue::ArraySlot(index) = value {
            self.references[*index as usize] += 1;
        }
    }

    fn release(&mut self, value: RuntimeValue) {
        let RuntimeValue::ArraySlot(index) = value else {
            return;
        };
        self.references[index as usize] -= 1;
        if self.references[index as usize] != 0 {
            return;
        }
        let start = index as usize * self.width as usize;
        for offset in start..start + self.lengths[index as usize] as usize {
            let child = core::mem::replace(&mut self.values[offset], RuntimeValue::Void);
            self.release(child);
        }
        self.lengths[index as usize] = 0;
        self.free.push(index);
    }
}

#[derive(Clone, Debug, Default)]
struct VmRegisters {
    ints: Vec<i32>,
    floats: Vec<f32>,
    bools: Vec<bool>,
    colors: Vec<Color>,
    refs: Vec<RuntimeValue>,
}

impl VmRegisters {
    fn reserve(&mut self, layout: SlotLayout) {
        reserve(&mut self.ints, layout.ints as usize);
        reserve(&mut self.floats, layout.floats as usize);
        reserve(&mut self.bools, layout.bools as usize);
        reserve(&mut self.colors, layout.colors as usize);
        reserve(&mut self.refs, layout.refs as usize);
    }

    fn prepare(&mut self, bytecode: &BytecodeProgram) {
        if self.ints.len() == bytecode.layout.ints as usize
            && self.floats.len() == bytecode.layout.floats as usize
            && self.bools.len() == bytecode.layout.bools as usize
            && self.colors.len() == bytecode.layout.colors as usize
            && self.refs.len() == bytecode.layout.refs as usize
        {
            return;
        }
        self.ints.clear();
        self.ints.resize(bytecode.layout.ints as usize, 0);
        self.floats.clear();
        self.floats.resize(bytecode.layout.floats as usize, 0.0);
        self.bools.clear();
        self.bools.resize(bytecode.layout.bools as usize, false);
        self.colors.clear();
        self.colors.resize(bytecode.layout.colors as usize, black());
        self.refs.clear();
        self.refs
            .resize(bytecode.layout.refs as usize, RuntimeValue::Void);
    }
}

fn reserve<T>(values: &mut Vec<T>, capacity: usize) {
    if values.capacity() < capacity {
        values.reserve_exact(capacity - values.len());
    }
}

#[cfg(test)]
mod workspace_capacity_tests {
    #[test]
    fn reserve_grows_from_capacity_even_when_most_slots_are_unused() {
        let mut values = alloc::vec::Vec::<u32>::with_capacity(16);
        values.push(1);
        let required = values.capacity() + 1;
        super::reserve(&mut values, required);
        assert!(values.capacity() >= required);
        assert_eq!(values.as_slice(), &[1]);
    }
}

fn bind_values(
    declarations: &[ParamDecl],
    cache: &mut DslBindCache,
    bound: &mut BoundParams,
    mut resolve: impl FnMut(&ParamDecl) -> Result<Value, RuntimeError>,
) -> Result<(), RuntimeError> {
    bound.values.clear();
    bound.values.reserve(declarations.len());
    for param in declarations {
        bound
            .values
            .push(bind_param_value(&param.ty, resolve(param)?, cache));
    }
    Ok(())
}

pub(crate) fn run_sample_effect(
    effect: &CompiledEffect,
    params: &BoundParams,
    context: &RunContext,
    workspace: &mut VmWorkspace,
) -> Result<Color, RuntimeError> {
    if effect.kind != EffectKind::Sample {
        return Err(RuntimeError::new("cannot sample generator effect"));
    }
    run_sample_program(&effect.bytecode, params, context, workspace, 0)
}

pub(super) fn run_sample_program(
    bytecode: &BytecodeProgram,
    params: &BoundParams,
    context: &RunContext,
    workspace: &mut VmWorkspace,
    entry: usize,
) -> Result<Color, RuntimeError> {
    let mut vm = Vm::new(
        bytecode,
        params,
        VmContext::Sample(context),
        workspace,
        None,
        None,
        entry,
    );
    vm.run_color()
}

pub(crate) fn run_generator_effect(
    effect: &CompiledEffect,
    params: &BoundParams,
    context: &GeneratorContext,
    workspace: &mut VmWorkspace,
) -> Result<Vec<GeneratedEffect>, RuntimeError> {
    if effect.kind != EffectKind::Generator {
        return Err(RuntimeError::new("cannot generate sample effect"));
    }
    let mut generated = Vec::new();
    let mut vm = Vm::new(
        &effect.bytecode,
        params,
        VmContext::Generator(context),
        workspace,
        None,
        Some((effect, &mut generated)),
        0,
    );
    let _ = vm.run()?;
    drop(vm);
    Ok(generated)
}

pub(super) fn evaluate_value(
    program: &BytecodeProgram,
    params: &BoundParams,
    context: &RunContext,
    workspace: &mut VmWorkspace,
    remaining_iterations: &mut usize,
) -> Result<Value, RuntimeError> {
    let mut vm = Vm::new(
        program,
        params,
        VmContext::Sample(context),
        workspace,
        None,
        None,
        0,
    );
    let result = vm.run()?;
    *remaining_iterations = remaining_iterations
        .checked_sub(vm.loop_iterations)
        .ok_or_else(|| RuntimeError::new("loop iteration limit exceeded"))?;
    Ok(runtime_to_value(
        result,
        vm.workspace.arrays.as_deref(),
        params.arrays.as_deref(),
    ))
}

pub(crate) fn run_operator(
    operator: &CompiledOperator,
    params: &BoundParams,
    context: &OperatorRunContext,
    sampler: &mut dyn SignalSampler,
    workspace: &mut VmWorkspace,
) -> Result<Color, RuntimeError> {
    run_operator_program(&operator.bytecode, params, context, sampler, workspace, 0)
}

pub(super) fn run_operator_program(
    bytecode: &BytecodeProgram,
    params: &BoundParams,
    context: &OperatorRunContext,
    sampler: &mut dyn SignalSampler,
    workspace: &mut VmWorkspace,
    entry: usize,
) -> Result<Color, RuntimeError> {
    let mut vm = Vm::new(
        bytecode,
        params,
        VmContext::Sample(context),
        workspace,
        Some(sampler),
        None,
        entry,
    );
    vm.run_color()
}

#[derive(Clone, Debug)]
enum RuntimeValue {
    Void,
    Int(i32),
    Float(f32),
    Bool(bool),
    Color(Color),
    Marks(Arc<Marks>),
    Timeline,
    Target(Arc<TargetValue>),
    TargetItems(Arc<TargetItemsValue>),
    TargetItem(Arc<TargetItemValue>),
    Curve(Arc<Curve>),
    Gradient(Arc<Gradient>),
    PreparedCurve(Arc<PreparedCurve>),
    Array(Arc<[Value]>),
    ArraySlot(u32),
    /// Borrowed from the invocation's immutable parameter arena.
    ParameterArray(u32),
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
            Value::Target(value) => Self::Target(Arc::clone(value)),
            Value::TargetItems(value) => Self::TargetItems(Arc::clone(value)),
            Value::TargetItem(value) => Self::TargetItem(Arc::clone(value)),
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
        RuntimeValue::Timeline => RuntimeValue::Timeline,
        RuntimeValue::Target(value) => RuntimeValue::Target(Arc::clone(value)),
        RuntimeValue::TargetItems(value) => RuntimeValue::TargetItems(Arc::clone(value)),
        RuntimeValue::TargetItem(value) => RuntimeValue::TargetItem(Arc::clone(value)),
        RuntimeValue::Curve(value) => RuntimeValue::Curve(Arc::clone(value)),
        RuntimeValue::Gradient(value) => RuntimeValue::Gradient(Arc::clone(value)),
        RuntimeValue::PreparedCurve(value) => RuntimeValue::PreparedCurve(Arc::clone(value)),
        RuntimeValue::Array(value) => RuntimeValue::Array(Arc::clone(value)),
        RuntimeValue::ArraySlot(index) => RuntimeValue::ArraySlot(*index),
        RuntimeValue::ParameterArray(index) => RuntimeValue::ParameterArray(*index),
        RuntimeValue::Enum(value) => RuntimeValue::Enum(value.clone()),
    }
}

pub(super) fn evaluate_bindings(
    program: &BytecodeProgram,
    params: &BoundParams,
    context: &RunContext,
    workspace: &mut VmWorkspace,
    output: &mut BoundParams,
    types: &[Type],
) -> Result<(), RuntimeError> {
    output.clear_results();
    let mut vm = Vm::new(
        program,
        params,
        VmContext::Sample(context),
        workspace,
        None,
        None,
        0,
    );
    let result = vm.run()?;
    let arrays = vm.workspace.arrays.as_deref();
    let parameter_arrays = params.arrays.as_deref();
    if output.len() != types.len()
        || array_length(&result, arrays, parameter_arrays)? != types.len()
    {
        return Err(RuntimeError::new(
            "parameter calculation returned an invalid output count",
        ));
    }
    for (index, ty) in types.iter().enumerate() {
        output.write_result(
            index,
            array_item(&result, index, arrays, parameter_arrays)?,
            ty,
            arrays,
            parameter_arrays,
        )?;
    }
    Ok(())
}

fn parameter_array_value(value: &RuntimeValue) -> RuntimeValue {
    match value {
        RuntimeValue::ArraySlot(index) => RuntimeValue::ParameterArray(*index),
        value => clone_runtime(value),
    }
}

fn array_length(
    value: &RuntimeValue,
    arrays: Option<&ArrayStorage>,
    parameters: Option<&ArrayStorage>,
) -> Result<usize, RuntimeError> {
    match value {
        RuntimeValue::Array(values) => return Ok(values.len()),
        RuntimeValue::ArraySlot(index) => {
            arrays.and_then(|arrays| arrays.lengths.get(*index as usize))
        }
        RuntimeValue::ParameterArray(index) => {
            parameters.and_then(|arrays| arrays.lengths.get(*index as usize))
        }
        _ => return Err(RuntimeError::new("expected array")),
    }
    .map(|length| *length as usize)
    .ok_or_else(|| RuntimeError::new("invalid calculated array"))
}

fn array_item(
    value: &RuntimeValue,
    index: usize,
    arrays: Option<&ArrayStorage>,
    parameters: Option<&ArrayStorage>,
) -> Result<RuntimeValue, RuntimeError> {
    if index >= array_length(value, arrays, parameters)? {
        return Err(RuntimeError::new("array index out of bounds"));
    }
    Ok(match value {
        RuntimeValue::Array(values) => RuntimeValue::from_value(&values[index]),
        RuntimeValue::ArraySlot(slot) => {
            clone_runtime(&arrays.expect("validated array storage").items(*slot)[index])
        }
        RuntimeValue::ParameterArray(slot) => parameter_array_value(
            &parameters
                .expect("validated parameter storage")
                .items(*slot)[index],
        ),
        _ => return Err(RuntimeError::new("expected array")),
    })
}

struct Vm<'a> {
    bytecode: &'a BytecodeProgram,
    params: &'a BoundParams,
    context: VmContext<'a>,
    workspace: &'a mut VmWorkspace,
    ip: usize,
    loop_iterations: usize,
    signal_sampler: Option<&'a mut (dyn SignalSampler + 'a)>,
    generated: Option<(&'a CompiledEffect, &'a mut Vec<GeneratedEffect>)>,
}

#[derive(Clone, Copy)]
enum VmContext<'a> {
    Sample(&'a RunContext),
    Generator(&'a GeneratorContext),
}

impl Drop for Vm<'_> {
    fn drop(&mut self) {
        // Local values must not keep parameter resources shared between invocations:
        // the next automation update needs exclusive access to its prepared curves.
        // Preserve register lengths/capacities so the next invocation reuses storage.
        // Assign directly: Clone-based slice fill produces unnecessary variant dispatch.
        for value in &mut self.workspace.registers.refs {
            let value = core::mem::replace(value, RuntimeValue::Void);
            if let Some(arrays) = &mut self.workspace.arrays {
                arrays.release(value);
            }
        }
    }
}

impl<'a> Vm<'a> {
    fn new(
        bytecode: &'a BytecodeProgram,
        params: &'a BoundParams,
        context: VmContext<'a>,
        workspace: &'a mut VmWorkspace,
        signal_sampler: Option<&'a mut (dyn SignalSampler + 'a)>,
        generated: Option<(&'a CompiledEffect, &'a mut Vec<GeneratedEffect>)>,
        entry: usize,
    ) -> Self {
        // A nonzero entry resumes a frame's initialized program/workspace.
        // Independent samples and each frame's first pixel always start at zero.
        if entry == 0 {
            workspace.registers.prepare(bytecode);
            if bytecode.array_capacity != 0 {
                workspace.reserve_arrays(bytecode);
            }
        }
        Self {
            bytecode,
            params,
            context,
            workspace,
            ip: entry,
            loop_iterations: 0,
            signal_sampler,
            generated,
        }
    }

    fn run_color(&mut self) -> Result<Color, RuntimeError> {
        match self.run()? {
            RuntimeValue::Color(color) => Ok(color),
            other => Err(RuntimeError::new(format!(
                "`sample` returned non-color value {other:?}"
            ))),
        }
    }

    fn run(&mut self) -> Result<RuntimeValue, RuntimeError> {
        loop {
            let Some(instruction) = self.bytecode.instructions.get(self.ip) else {
                return Err(RuntimeError::new("function completed without return"));
            };
            self.ip += 1;
            match instruction {
                Instruction::LoadConst { dst, constant } => {
                    let value = self
                        .bytecode
                        .constants
                        .get(*constant)
                        .ok_or_else(|| RuntimeError::new("invalid constant slot"))?;
                    self.set_const_value(*dst, value)?;
                }
                Instruction::LoadIntParam { dst, param } => {
                    self.load_int_param(*dst, *param)?;
                }
                Instruction::LoadFloatParam { dst, param } => {
                    self.load_float_param(*dst, *param)?;
                }
                Instruction::LoadBoolParam { dst, param } => {
                    self.load_bool_param(*dst, *param)?;
                }
                Instruction::LoadColorParam { dst, param } => {
                    self.load_color_param(*dst, *param)?;
                }
                Instruction::LoadRefParam { dst, param } => {
                    self.load_ref_param(*dst, *param)?;
                }
                Instruction::LoadGeneratorContext { dst, slot } => {
                    let value = self.generator_context_value(*slot)?;
                    self.set_value(*dst, value)?;
                }
                Instruction::Move { dst, src } => {
                    self.copy_slot(*dst, *src)?;
                }
                Instruction::MakeArray { dst, items } => {
                    let items = self
                        .bytecode
                        .value_operands(*items)
                        .ok_or_else(|| RuntimeError::new("invalid value operand span"))?;
                    let index = self
                        .workspace
                        .arrays
                        .as_mut()
                        .ok_or_else(|| RuntimeError::new("missing prepared array storage"))?
                        .allocate(items.len())?;
                    let result = (|| {
                        for (offset, item) in items.iter().enumerate() {
                            let value = self.value(*item)?;
                            let arrays = self
                                .workspace
                                .arrays
                                .as_mut()
                                .expect("prepared array storage");
                            arrays.retain(&value);
                            arrays.values[index as usize * arrays.width as usize + offset] = value;
                        }
                        self.set_ref(*dst, RuntimeValue::ArraySlot(index))
                    })();
                    self.workspace
                        .arrays
                        .as_mut()
                        .expect("prepared array storage")
                        .release(RuntimeValue::ArraySlot(index));
                    result?;
                }
                Instruction::Index { dst, target, index } => {
                    let target = self.ref_value(*target)?;
                    let index = self.value(*index)?;
                    let value = self.index_value(target, &index)?;
                    self.set_value(*dst, value)?;
                }
                Instruction::Select { dst, items, index } => {
                    let index = self.value(*index)?;
                    let index = usize::try_from(to_int_runtime(&index, self.params)?)
                        .map_err(|_| RuntimeError::new("array index cannot be negative"))?;
                    let source = self
                        .bytecode
                        .value_operands(*items)
                        .ok_or_else(|| RuntimeError::new("invalid value operand span"))?
                        .get(index)
                        .copied()
                        .ok_or_else(|| RuntimeError::new("array index out of bounds"))?;
                    self.copy_slot(*dst, source)?;
                }
                Instruction::CurveParamSample {
                    dst,
                    param,
                    position,
                } => {
                    let position = self.float(*position)?;
                    self.set_float(*dst, self.params.sample_curve(*param, position)?)?;
                }
                Instruction::GradientParamSample {
                    dst,
                    param,
                    position,
                } => {
                    let position = self.float(*position)?;
                    let color = sample_gradient(self.prepared_gradient_param(*param)?, position)?;
                    self.set_color(*dst, color)?;
                }
                Instruction::SignalSample {
                    dst,
                    input,
                    seconds,
                    pixel,
                    frame_cache,
                } => {
                    let seconds = self.float(*seconds)?;
                    let pixel = match *pixel {
                        SignalPixel::Current => SignalPixel::Current,
                        SignalPixel::Local(index) => SignalPixel::Local(self.int(index)?),
                        SignalPixel::Global(index) => SignalPixel::Global(self.int(index)?),
                    };
                    let color = match crate::values::sample_time_from_seconds_f32(seconds) {
                        Ok(sample_time) => self
                            .signal_sampler
                            .as_deref_mut()
                            .ok_or_else(|| RuntimeError::new("Signal sampler is unavailable"))?
                            .sample_signal(
                                *input,
                                sample_time,
                                pixel,
                                (*frame_cache != u32::MAX).then_some(*frame_cache as usize),
                            )?,
                        Err(SampleTimeError::Negative) => black(),
                        Err(_) => {
                            return Err(RuntimeError::new("Signal sample time is out of range"));
                        }
                    };
                    self.set_color(*dst, color)?;
                }
                Instruction::Member {
                    dst,
                    target,
                    member,
                } => {
                    let value = member_value(self.ref_value(*target)?, member)?;
                    self.set_value(*dst, value)?;
                }
                Instruction::IntToFloat { dst, src } => {
                    self.set_float(*dst, self.int(*src)? as f32)?;
                }
                Instruction::Not { dst, src } => self.set_bool(*dst, !self.bool(*src)?)?,
                Instruction::NegInt { dst, src } => {
                    let value = self
                        .int(*src)?
                        .checked_neg()
                        .ok_or_else(|| RuntimeError::new("integer negation overflow"))?;
                    self.set_int(*dst, value)?;
                }
                Instruction::NegFloat { dst, src } => {
                    self.set_float(*dst, -self.float(*src)?)?;
                }
                Instruction::FloatArithmetic {
                    dst,
                    op,
                    left,
                    right,
                } => {
                    let left = self.float(*left)?;
                    let right = self.float(*right)?;
                    let value = match op {
                        ArithmeticOp::Add => left + right,
                        ArithmeticOp::Subtract => left - right,
                        ArithmeticOp::Multiply => left * right,
                        ArithmeticOp::Divide => left / right,
                        ArithmeticOp::Remainder => left % right,
                    };
                    self.set_float(*dst, value)?;
                }
                Instruction::FloatArithmeticConst {
                    dst,
                    op,
                    value,
                    constant_bits,
                    constant_left,
                } => {
                    let value = self.float(*value)?;
                    let constant = f32::from_bits(*constant_bits);
                    let (left, right) = if *constant_left {
                        (constant, value)
                    } else {
                        (value, constant)
                    };
                    let value = match op {
                        ArithmeticOp::Add => left + right,
                        ArithmeticOp::Subtract => left - right,
                        ArithmeticOp::Multiply => left * right,
                        ArithmeticOp::Divide => left / right,
                        ArithmeticOp::Remainder => left % right,
                    };
                    self.set_float(*dst, value)?;
                }
                Instruction::IntArithmetic {
                    dst,
                    op,
                    left,
                    right,
                } => {
                    let left = self.int(*left)?;
                    let right = self.int(*right)?;
                    let value = match op {
                        IntArithmeticOp::Add => left.checked_add(right),
                        IntArithmeticOp::Subtract => left.checked_sub(right),
                        IntArithmeticOp::Multiply => left.checked_mul(right),
                        IntArithmeticOp::Remainder => left.checked_rem(right),
                    };
                    let value = value.ok_or_else(|| {
                        RuntimeError::new("integer arithmetic overflow or division by zero")
                    })?;
                    self.set_int(*dst, value)?;
                }
                Instruction::FloatCompare {
                    dst,
                    op,
                    left,
                    right,
                } => {
                    let left = self.float(*left)?;
                    let right = self.float(*right)?;
                    let value = match op {
                        CompareOp::Less => left < right,
                        CompareOp::LessEqual => left <= right,
                        CompareOp::Greater => left > right,
                        CompareOp::GreaterEqual => left >= right,
                    };
                    self.set_bool(*dst, value)?;
                }
                Instruction::FloatCompareConst {
                    dst,
                    op,
                    value,
                    constant_bits,
                    constant_left,
                } => {
                    let value = self.float(*value)?;
                    let constant = f32::from_bits(*constant_bits);
                    let (left, right) = if *constant_left {
                        (constant, value)
                    } else {
                        (value, constant)
                    };
                    let value = match op {
                        CompareOp::Less => left < right,
                        CompareOp::LessEqual => left <= right,
                        CompareOp::Greater => left > right,
                        CompareOp::GreaterEqual => left >= right,
                    };
                    self.set_bool(*dst, value)?;
                }
                Instruction::ValueEqual {
                    dst,
                    negate,
                    left,
                    right,
                } => {
                    let equal = self.slots_equal(*left, *right)?;
                    self.set_bool(*dst, if *negate { !equal } else { equal })?;
                }
                Instruction::EnumParamEqualConst {
                    dst,
                    param,
                    constant,
                    negate,
                } => {
                    let equal = self.enum_param_equal_const(*param, *constant)?;
                    self.set_bool(*dst, if *negate { !equal } else { equal })?;
                }
                Instruction::Jump(target) => self.ip = *target,
                Instruction::JumpIfFalse { condition, target } => {
                    if !self.bool(*condition)? {
                        self.ip = *target;
                    }
                }
                Instruction::JumpIfTrue { condition, target } => {
                    if self.bool(*condition)? {
                        self.ip = *target;
                    }
                }
                Instruction::ContextRead { dst, read } => {
                    self.context_read(*dst, *read)?;
                }
                Instruction::SectionPosition { dst, width } => {
                    let width = self.float(*width)?.max(1.0);
                    let index = sample_context(self.context)?.pixel_index as f32;
                    self.set_float(*dst, (index - libm::floorf(index / width) * width) / width)?;
                }
                Instruction::FloatUnary { dst, op, value } => {
                    let value = self.float(*value)?;
                    let result = match op {
                        FloatUnary::Sin => micromath::F32Ext::sin(value),
                        FloatUnary::Cos => micromath::F32Ext::cos(value),
                        FloatUnary::Abs => value.abs(),
                        FloatUnary::Floor => libm::floorf(value),
                    };
                    self.set_float(*dst, result)?;
                }
                Instruction::FloatBinary {
                    dst,
                    op,
                    left,
                    right,
                } => {
                    let left = self.float(*left)?;
                    let right = self.float(*right)?;
                    let result = match op {
                        FloatBinary::Min => left.min(right),
                        FloatBinary::Max => left.max(right),
                    };
                    self.set_float(*dst, result)?;
                }
                Instruction::FloatBinaryConst {
                    dst,
                    op,
                    value,
                    constant_bits,
                } => {
                    let value = self.float(*value)?;
                    let constant = f32::from_bits(*constant_bits);
                    let result = match op {
                        FloatBinary::Min => value.min(constant),
                        FloatBinary::Max => value.max(constant),
                    };
                    self.set_float(*dst, result)?;
                }
                Instruction::Clamp {
                    dst,
                    value,
                    min,
                    max,
                } => {
                    let value = self.float(*value)?;
                    let min = self.float(*min)?;
                    let max = self.float(*max)?;
                    self.set_float(*dst, value.clamp(min, max))?;
                }
                Instruction::ClampConst {
                    dst,
                    value,
                    min_bits,
                    max_bits,
                } => {
                    let value = self.float(*value)?;
                    self.set_float(
                        *dst,
                        value.clamp(f32::from_bits(*min_bits), f32::from_bits(*max_bits)),
                    )?;
                }
                Instruction::Smoothstep {
                    dst,
                    edge0,
                    edge1,
                    value,
                } => {
                    let edge0 = self.float(*edge0)?;
                    let edge1 = self.float(*edge1)?;
                    let value = self.float(*value)?;
                    let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
                    self.set_float(*dst, t * t * (3.0 - 2.0 * t))?;
                }
                Instruction::MixFloat {
                    dst,
                    left,
                    right,
                    amount,
                } => {
                    let amount = self.float(*amount)?;
                    let left = self.float(*left)?;
                    let right = self.float(*right)?;
                    self.set_float(*dst, left + (right - left) * amount)?;
                }
                Instruction::MixColor {
                    dst,
                    left,
                    right,
                    amount,
                } => {
                    let amount = self.float(*amount)?;
                    let left = self.color(*left)?;
                    let right = self.color(*right)?;
                    self.set_color(*dst, mix_colors(left, right, amount))?;
                }
                Instruction::ColorBinary {
                    dst,
                    op,
                    left,
                    right,
                } => {
                    let left = self.color(*left)?;
                    let right = self.color(*right)?;
                    let color = match op {
                        ColorBinary::Add => add_colors(left, right),
                        ColorBinary::Multiply => multiply_colors(left, right),
                        ColorBinary::Max => max_colors(left, right),
                    };
                    self.set_color(*dst, color)?;
                }
                Instruction::ColorScale { dst, color, scale } => {
                    let color = self.color(*color)?;
                    let scale = self.float(*scale)?;
                    self.set_color(*dst, scale_color(color, scale))?;
                }
                Instruction::ColorIntensity { dst, color } => {
                    let color = self.color(*color)?;
                    self.set_float(*dst, color_intensity(color))?;
                }
                Instruction::ColorInvert { dst, color } => {
                    let color = self.color(*color)?;
                    self.set_color(*dst, invert_color(color))?;
                }
                Instruction::Rgb {
                    dst,
                    red,
                    green,
                    blue,
                } => {
                    self.set_color(
                        *dst,
                        Color {
                            red: channel(self.float(*red)?),
                            green: channel(self.float(*green)?),
                            blue: channel(self.float(*blue)?),
                        },
                    )?;
                }
                Instruction::Hsv {
                    dst,
                    hue,
                    saturation,
                    value,
                } => {
                    self.set_color(
                        *dst,
                        crate::sampling::hsv(
                            self.float(*hue)?,
                            self.float(*saturation)?,
                            self.float(*value)?,
                        ),
                    )?;
                }
                Instruction::Rand { dst, args } => {
                    let args = self
                        .bytecode
                        .value_operands(*args)
                        .ok_or_else(|| RuntimeError::new("invalid random operand span"))?;
                    let random = self.random(args)?;
                    self.set_float(*dst, random)?;
                }
                Instruction::CurveFloatClamped {
                    dst,
                    curve,
                    position,
                    min,
                    max,
                } => {
                    let curve = to_curve_runtime(self.ref_value(*curve)?, self.params)?;
                    let position = self.float(*position)?;
                    let min = self.float(*min)?;
                    let max = self.float(*max)?;
                    self.set_float(*dst, sample_curve(curve, position).clamp(min, max))?;
                }
                Instruction::CurveParamFloatClamped {
                    dst,
                    param,
                    position,
                    min,
                    max,
                } => {
                    let position = self.float(*position)?;
                    let min = self.float(*min)?;
                    let max = self.float(*max)?;
                    let value = self.params.sample_curve(*param, position)?.clamp(min, max);
                    self.set_float(*dst, value)?;
                }
                Instruction::GradientColorScaled {
                    dst,
                    gradient,
                    position,
                    scale,
                } => {
                    let scale = self.float(*scale)?.clamp(0.0, 1.0);
                    if scale <= 0.0 {
                        self.set_color(*dst, black())?;
                    } else {
                        let gradient =
                            to_gradient_runtime(self.ref_value(*gradient)?, self.params)?;
                        let position = self.float(*position)?;
                        let color = sample_gradient(gradient, position)?;
                        self.set_color(*dst, scale_color(color, scale))?;
                    }
                }
                Instruction::GradientParamColorScaled {
                    dst,
                    param,
                    position,
                    scale,
                } => {
                    let scale = self.float(*scale)?.clamp(0.0, 1.0);
                    if scale <= 0.0 {
                        self.set_color(*dst, black())?;
                    } else {
                        let position = self.float(*position)?;
                        let gradient = self.prepared_gradient_param(*param)?;
                        let color = sample_gradient(gradient, position)?;
                        self.set_color(*dst, scale_color(color, scale))?;
                    }
                }
                Instruction::CurveCrossing {
                    dst,
                    curve,
                    value,
                    fallback,
                } => {
                    let curve = to_curve_runtime(self.ref_value(*curve)?, self.params)?;
                    let value = self.float(*value)?;
                    let fallback = fallback
                        .map(|fallback| self.float(fallback))
                        .transpose()?
                        .unwrap_or(value);
                    self.set_float(*dst, curve_crossing_raw(curve, value, fallback))?;
                }
                Instruction::CurveParamCrossing {
                    dst,
                    param,
                    value,
                    fallback,
                } => {
                    let value = self.float(*value)?;
                    let fallback = fallback
                        .map(|fallback| self.float(fallback))
                        .transpose()?
                        .unwrap_or(value);
                    self.set_float(*dst, self.params.curve_crossing(*param, value, fallback)?)?;
                }
                Instruction::Len { dst, value } => {
                    let value = match self.ref_value(*value)? {
                        value @ (RuntimeValue::Array(_)
                        | RuntimeValue::ArraySlot(_)
                        | RuntimeValue::ParameterArray(_)) => i32::try_from(array_length(
                            value,
                            self.workspace.arrays.as_deref(),
                            self.params.arrays.as_deref(),
                        )?)
                        .map_err(|_| RuntimeError::new("array length exceeds int range"))?,
                        RuntimeValue::Marks(marks) => i32::try_from(marks.marks.len())
                            .map_err(|_| RuntimeError::new("mark count exceeds int range"))?,
                        _ => return Err(RuntimeError::new("len requires array or marks")),
                    };
                    self.set_int(*dst, value)?;
                }
                Instruction::Mark { dst, op, args } => {
                    let args = self
                        .bytecode
                        .value_operands(*args)
                        .ok_or_else(|| RuntimeError::new("invalid mark operand span"))?;
                    match op {
                        MarkOp::Count => {
                            let marks = self.mark_arg(args, 0)?;
                            let value = i32::try_from(marks.marks.len())
                                .map_err(|_| RuntimeError::new("mark count exceeds int range"))?;
                            self.set_int(self.int_slot_value(*dst)?, value)?;
                        }
                        MarkOp::At => {
                            let marks = self.mark_arg(args, 0)?;
                            let index = self.int_arg(args, 1)?;
                            let fallback = self.optional_float_arg(args, 2)?.unwrap_or(0.0);
                            let value = mark_at_from(marks, index, fallback)?;
                            self.set_float(self.float_slot_value(*dst)?, value)?;
                        }
                        MarkOp::Prev => {
                            let marks = self.mark_arg(args, 0)?;
                            let seconds = self.optional_float_arg(args, 1)?.unwrap_or(
                                sample_context(self.context)
                                    .map(|context| sample_duration_seconds_f32(context.time))
                                    .unwrap_or(0.0),
                            );
                            let fallback = self.optional_float_arg(args, 2)?.unwrap_or(0.0);
                            let index = prev_index(marks, seconds)?;
                            let value = if index < 0 {
                                fallback
                            } else {
                                mark_at_from(marks, index, fallback)?
                            };
                            self.set_float(self.float_slot_value(*dst)?, value)?;
                        }
                        MarkOp::PrevIndex => {
                            let marks = self.mark_arg(args, 0)?;
                            let seconds = self.optional_float_arg(args, 1)?.unwrap_or(
                                sample_context(self.context)
                                    .map(|context| sample_duration_seconds_f32(context.time))
                                    .unwrap_or(0.0),
                            );
                            let value = prev_index(marks, seconds)?;
                            self.set_int(self.int_slot_value(*dst)?, value)?;
                        }
                        MarkOp::NextIndex => {
                            let marks = self.mark_arg(args, 0)?;
                            let seconds = self.optional_float_arg(args, 1)?.unwrap_or(
                                sample_context(self.context)
                                    .map(|context| sample_duration_seconds_f32(context.time))
                                    .unwrap_or(0.0),
                            );
                            let value = next_index(marks, seconds)?;
                            self.set_int(self.int_slot_value(*dst)?, value)?;
                        }
                        MarkOp::Elapsed => {
                            let marks = self.mark_arg(args, 0)?;
                            let seconds = self.optional_float_arg(args, 1)?.unwrap_or(
                                sample_context(self.context)
                                    .map(|context| sample_duration_seconds_f32(context.time))
                                    .unwrap_or(0.0),
                            );
                            let value = elapsed(marks, seconds)?;
                            self.set_float(self.float_slot_value(*dst)?, value)?;
                        }
                        MarkOp::Phase => {
                            let marks = self.mark_arg(args, 0)?;
                            let seconds = self.optional_float_arg(args, 1)?.unwrap_or(
                                sample_context(self.context)
                                    .map(|context| sample_duration_seconds_f32(context.time))
                                    .unwrap_or(0.0),
                            );
                            let duration = match self.context {
                                VmContext::Sample(context) => {
                                    sample_duration_seconds_f32(context.duration)
                                }
                                VmContext::Generator(context) => {
                                    sample_duration_seconds_f32(context.duration)
                                }
                            };
                            let value = phase(marks, seconds, duration)?;
                            self.set_float(self.float_slot_value(*dst)?, value)?;
                        }
                    }
                }
                Instruction::TargetItems { dst, op, args } => {
                    let args = self
                        .bytecode
                        .value_operands(*args)
                        .ok_or_else(|| RuntimeError::new("invalid target operand span"))?;
                    match op {
                        TargetItemsOp::Fixtures => {
                            let target = self.ref_arg(args, 0)?;
                            self.set_value(
                                *dst,
                                RuntimeValue::TargetItems(Arc::new(fixtures(target)?)),
                            )?
                        }
                        TargetItemsOp::Pixels => {
                            let target = self.ref_arg(args, 0)?;
                            self.set_value(
                                *dst,
                                RuntimeValue::TargetItems(Arc::new(pixels(target)?)),
                            )?
                        }
                        TargetItemsOp::Sections => {
                            let target = self.ref_arg(args, 0)?;
                            let width = self.float_arg(args, 1)?;
                            self.set_value(
                                *dst,
                                RuntimeValue::TargetItems(Arc::new(sections(target, width)?)),
                            )?
                        }
                        TargetItemsOp::Count => {
                            let target = self.ref_arg(args, 0)?;
                            self.set_value(
                                *dst,
                                RuntimeValue::Int(target_items(target)?.groups.len() as i32),
                            )?
                        }
                        TargetItemsOp::Pick => {
                            let target = self.ref_arg(args, 0)?;
                            let items = target_items(target)?;
                            let index = usize::try_from(self.int_arg(args, 1)?).map_err(|_| {
                                RuntimeError::new("target item index cannot be negative")
                            })?;
                            self.set_value(
                                *dst,
                                RuntimeValue::TargetItem(
                                    items.groups.get(index).cloned().ok_or_else(|| {
                                        RuntimeError::new("target item index out of bounds")
                                    })?,
                                ),
                            )?;
                        }
                    }
                }
                Instruction::CheckLoopLimit => {
                    self.loop_iterations += 1;
                    if self.loop_iterations > MAX_VM_INSTRUCTIONS_PER_INVOCATION {
                        return Err(RuntimeError::new("loop iteration limit exceeded"));
                    }
                }
                Instruction::Emit { effect, fields } => {
                    let definition = self
                        .generated
                        .as_ref()
                        .ok_or_else(|| RuntimeError::new("emit is only valid in a generator"))?
                        .0;
                    if effect.0 >= definition.generated_effect_count {
                        return Err(RuntimeError::new("invalid generated effect slot"));
                    }
                    let fields = definition
                        .emit_fields
                        .get(fields.range())
                        .ok_or_else(|| RuntimeError::new("invalid emit field span"))?;
                    self.emit_generated(*effect, fields)?;
                }
                Instruction::Return(src) => return self.return_value(*src),
                Instruction::ReturnColor(src) => return Ok(RuntimeValue::Color(self.color(*src)?)),
            }
        }
    }

    fn return_value(&self, slot: ValueSlot) -> Result<RuntimeValue, RuntimeError> {
        self.value(slot)
    }

    fn int(&self, slot: IntSlot) -> Result<i32, RuntimeError> {
        Ok(self.workspace.registers.ints[slot.0 as usize])
    }

    fn float(&self, slot: FloatSlot) -> Result<f32, RuntimeError> {
        Ok(self.workspace.registers.floats[slot.0 as usize])
    }

    fn bool(&self, slot: BoolSlot) -> Result<bool, RuntimeError> {
        Ok(self.workspace.registers.bools[slot.0 as usize])
    }

    fn color(&self, slot: ColorSlot) -> Result<Color, RuntimeError> {
        Ok(self.workspace.registers.colors[slot.0 as usize])
    }

    fn ref_value(&self, slot: RefSlot) -> Result<&RuntimeValue, RuntimeError> {
        Ok(&self.workspace.registers.refs[slot.0 as usize])
    }

    fn value(&self, slot: ValueSlot) -> Result<RuntimeValue, RuntimeError> {
        match slot {
            ValueSlot::Int(slot) => self.int(slot).map(RuntimeValue::Int),
            ValueSlot::Float(slot) => self.float(slot).map(RuntimeValue::Float),
            ValueSlot::Bool(slot) => self.bool(slot).map(RuntimeValue::Bool),
            ValueSlot::Color(slot) => self.color(slot).map(RuntimeValue::Color),
            ValueSlot::Ref(slot) => self.ref_value(slot).map(clone_runtime),
        }
    }

    fn float_value(&self, slot: ValueSlot) -> Result<f32, RuntimeError> {
        match slot {
            ValueSlot::Int(slot) => self.int(slot).map(|value| value as f32),
            ValueSlot::Float(slot) => self.float(slot),
            _ => Err(RuntimeError::new("expected float")),
        }
    }

    fn target_item_value(&self, slot: ValueSlot) -> Result<Arc<TargetItemValue>, RuntimeError> {
        let ValueSlot::Ref(slot) = slot else {
            return Err(RuntimeError::new("emit target must be target items"));
        };
        match self.ref_value(slot)? {
            RuntimeValue::TargetItem(item) => Ok(Arc::clone(item)),
            RuntimeValue::TargetItems(items) => Ok(target_item_from_groups(&items.groups)),
            RuntimeValue::Target(target) => Ok(target_item_from_groups(&target.groups)),
            _ => Err(RuntimeError::new("emit target must be target items")),
        }
    }

    fn int_slot_value(&self, slot: ValueSlot) -> Result<IntSlot, RuntimeError> {
        match slot {
            ValueSlot::Int(slot) => Ok(slot),
            _ => Err(RuntimeError::new("expected int slot")),
        }
    }

    fn float_slot_value(&self, slot: ValueSlot) -> Result<FloatSlot, RuntimeError> {
        match slot {
            ValueSlot::Float(slot) => Ok(slot),
            _ => Err(RuntimeError::new("expected float slot")),
        }
    }

    fn mark_arg(&self, args: &[ValueSlot], index: usize) -> Result<&Marks, RuntimeError> {
        let Some(slot) = args.get(index) else {
            return Err(RuntimeError::new("missing argument"));
        };
        let ValueSlot::Ref(slot) = slot else {
            return Err(RuntimeError::new("mark builtin first arg must be marks"));
        };
        match self.ref_value(*slot)? {
            RuntimeValue::Marks(marks) => Ok(marks),
            _ => Err(RuntimeError::new("mark builtin first arg must be marks")),
        }
    }

    fn int_arg(&self, args: &[ValueSlot], index: usize) -> Result<i32, RuntimeError> {
        let Some(slot) = args.get(index) else {
            return Err(RuntimeError::new("missing argument"));
        };
        match *slot {
            ValueSlot::Int(slot) => self.int(slot),
            ValueSlot::Float(slot) => self.float(slot).map(|value| value as i32),
            _ => Err(RuntimeError::new("expected int")),
        }
    }

    fn float_arg(&self, args: &[ValueSlot], index: usize) -> Result<f32, RuntimeError> {
        let Some(slot) = args.get(index) else {
            return Err(RuntimeError::new("missing argument"));
        };
        match *slot {
            ValueSlot::Int(slot) => self.int(slot).map(|value| value as f32),
            ValueSlot::Float(slot) => self.float(slot),
            _ => Err(RuntimeError::new("expected float")),
        }
    }

    fn optional_float_arg(
        &self,
        args: &[ValueSlot],
        index: usize,
    ) -> Result<Option<f32>, RuntimeError> {
        let Some(slot) = args.get(index) else {
            return Ok(None);
        };
        match *slot {
            ValueSlot::Int(slot) => self.int(slot).map(|value| Some(value as f32)),
            ValueSlot::Float(slot) => self.float(slot).map(Some),
            _ => Err(RuntimeError::new("expected float")),
        }
    }

    fn ref_arg(&self, args: &[ValueSlot], index: usize) -> Result<&RuntimeValue, RuntimeError> {
        let Some(slot) = args.get(index) else {
            return Err(RuntimeError::new("missing argument"));
        };
        let ValueSlot::Ref(slot) = *slot else {
            return Err(RuntimeError::new("expected reference-like value"));
        };
        self.ref_value(slot)
    }

    fn set_int(&mut self, slot: IntSlot, value: i32) -> Result<(), RuntimeError> {
        self.workspace.registers.ints[slot.0 as usize] = value;
        Ok(())
    }

    fn set_float(&mut self, slot: FloatSlot, value: f32) -> Result<(), RuntimeError> {
        self.workspace.registers.floats[slot.0 as usize] = value;
        Ok(())
    }

    fn set_bool(&mut self, slot: BoolSlot, value: bool) -> Result<(), RuntimeError> {
        self.workspace.registers.bools[slot.0 as usize] = value;
        Ok(())
    }

    fn set_color(&mut self, slot: ColorSlot, value: Color) -> Result<(), RuntimeError> {
        self.workspace.registers.colors[slot.0 as usize] = value;
        Ok(())
    }

    fn set_ref(&mut self, slot: RefSlot, value: RuntimeValue) -> Result<(), RuntimeError> {
        if let Some(arrays) = &mut self.workspace.arrays {
            arrays.retain(&value);
        }
        let old = core::mem::replace(&mut self.workspace.registers.refs[slot.0 as usize], value);
        if let Some(arrays) = &mut self.workspace.arrays {
            arrays.release(old);
        }
        Ok(())
    }

    fn set_value(&mut self, slot: ValueSlot, value: RuntimeValue) -> Result<(), RuntimeError> {
        match (slot, value) {
            (ValueSlot::Int(slot), RuntimeValue::Int(value)) => self.set_int(slot, value),
            (ValueSlot::Int(slot), RuntimeValue::Float(value)) => self.set_int(slot, value as i32),
            (ValueSlot::Float(slot), RuntimeValue::Float(value)) => self.set_float(slot, value),
            (ValueSlot::Float(slot), RuntimeValue::Int(value)) => {
                self.set_float(slot, value as f32)
            }
            (ValueSlot::Bool(slot), RuntimeValue::Bool(value)) => self.set_bool(slot, value),
            (ValueSlot::Color(slot), RuntimeValue::Color(value)) => self.set_color(slot, value),
            (ValueSlot::Ref(slot), value) => self.set_ref(slot, value),
            _ => Err(RuntimeError::new("type mismatch writing VM slot")),
        }
    }

    fn set_const_value(&mut self, slot: ValueSlot, value: &Value) -> Result<(), RuntimeError> {
        match (slot, value) {
            (ValueSlot::Int(slot), Value::Int(value)) => self.set_int(slot, *value),
            (ValueSlot::Float(slot), Value::Float(value)) => self.set_float(slot, *value),
            (ValueSlot::Float(slot), Value::Int(value)) => self.set_float(slot, *value as f32),
            (ValueSlot::Bool(slot), Value::Bool(value)) => self.set_bool(slot, *value),
            (ValueSlot::Color(slot), Value::Color(value)) => self.set_color(slot, *value),
            (ValueSlot::Ref(slot), value) => self.set_ref(slot, RuntimeValue::from_value(value)),
            _ => self.set_value(slot, RuntimeValue::from_value(value)),
        }
    }

    fn load_int_param(&mut self, slot: IntSlot, index: usize) -> Result<(), RuntimeError> {
        match self.params.values.get(index) {
            Some(BoundParamValue::Int(value)) => self.set_int(slot, *value),
            Some(BoundParamValue::Float(value)) => self.set_int(slot, *value as i32),
            Some(_) => Err(RuntimeError::new("expected int")),
            None => Err(RuntimeError::new("invalid param slot")),
        }
    }

    fn load_float_param(&mut self, slot: FloatSlot, index: usize) -> Result<(), RuntimeError> {
        match self.params.values.get(index) {
            Some(BoundParamValue::Float(value)) => self.set_float(slot, *value),
            Some(BoundParamValue::Int(value)) => self.set_float(slot, *value as f32),
            Some(_) => Err(RuntimeError::new("expected float")),
            None => Err(RuntimeError::new("invalid param slot")),
        }
    }

    fn load_bool_param(&mut self, slot: BoolSlot, index: usize) -> Result<(), RuntimeError> {
        match self.params.values.get(index) {
            Some(BoundParamValue::Bool(value)) => self.set_bool(slot, *value),
            Some(_) => Err(RuntimeError::new("expected bool")),
            None => Err(RuntimeError::new("invalid param slot")),
        }
    }

    fn load_color_param(&mut self, slot: ColorSlot, index: usize) -> Result<(), RuntimeError> {
        match self.params.values.get(index) {
            Some(BoundParamValue::Color(value)) => self.set_color(slot, *value),
            Some(_) => Err(RuntimeError::new("expected color")),
            None => Err(RuntimeError::new("invalid param slot")),
        }
    }

    fn load_ref_param(&mut self, slot: RefSlot, index: usize) -> Result<(), RuntimeError> {
        match self.params.values.get(index) {
            Some(value) => self.set_ref(slot, value.to_runtime()),
            None => Err(RuntimeError::new("invalid param slot")),
        }
    }

    fn copy_slot(&mut self, dst: ValueSlot, src: ValueSlot) -> Result<(), RuntimeError> {
        match (dst, src) {
            (ValueSlot::Int(dst), ValueSlot::Int(src)) => self.set_int(dst, self.int(src)?),
            (ValueSlot::Int(dst), ValueSlot::Float(src)) => {
                self.set_int(dst, self.float(src)? as i32)
            }
            (ValueSlot::Float(dst), ValueSlot::Float(src)) => self.set_float(dst, self.float(src)?),
            (ValueSlot::Float(dst), ValueSlot::Int(src)) => {
                self.set_float(dst, self.int(src)? as f32)
            }
            (ValueSlot::Bool(dst), ValueSlot::Bool(src)) => self.set_bool(dst, self.bool(src)?),
            (ValueSlot::Color(dst), ValueSlot::Color(src)) => self.set_color(dst, self.color(src)?),
            (ValueSlot::Ref(dst), ValueSlot::Ref(src)) => {
                self.set_ref(dst, clone_runtime(self.ref_value(src)?))
            }
            _ => Err(RuntimeError::new("type mismatch copying VM slot")),
        }
    }

    fn random(&self, args: &[ValueSlot]) -> Result<f32, RuntimeError> {
        let mut seed = 0.0;
        for slot in args {
            let ValueSlot::Float(slot) = slot else {
                return Err(RuntimeError::new("random operand is not a float"));
            };
            seed = seed * 31.0 + self.workspace.registers.floats[slot.0 as usize];
        }
        Ok(crate::sampling::deterministic_random_seed(seed))
    }

    fn generator_context_value(
        &self,
        slot: GeneratorContextId,
    ) -> Result<RuntimeValue, RuntimeError> {
        let VmContext::Generator(context) = self.context else {
            return Err(RuntimeError::new("generator context is unavailable"));
        };
        Ok(match slot {
            GeneratorContextId::Timeline => RuntimeValue::Timeline,
            GeneratorContextId::Target => RuntimeValue::Target(Arc::clone(&context.target)),
            GeneratorContextId::Duration => {
                RuntimeValue::Float(sample_duration_seconds_f32(context.duration))
            }
        })
    }

    fn context_read(&mut self, dst: ValueSlot, read: ContextRead) -> Result<(), RuntimeError> {
        match read {
            ContextRead::Progress => {
                self.set_context_float(dst, sample_context(self.context)?.progress)
            }
            ContextRead::Seconds => self.set_context_float(
                dst,
                sample_duration_seconds_f32(sample_context(self.context)?.time),
            ),
            ContextRead::Duration => self.set_context_float(
                dst,
                match self.context {
                    VmContext::Sample(context) => sample_duration_seconds_f32(context.duration),
                    VmContext::Generator(context) => sample_duration_seconds_f32(context.duration),
                },
            ),
            ContextRead::PixelIndex => {
                self.set_context_int(dst, sample_context(self.context)?.pixel_index)
            }
            ContextRead::PixelCount => {
                self.set_context_int(dst, sample_context(self.context)?.pixel_count)
            }
            ContextRead::PixelFraction => {
                self.set_context_float(dst, sample_context(self.context)?.pixel_fraction)
            }
        }
    }

    fn set_context_float(&mut self, dst: ValueSlot, value: f32) -> Result<(), RuntimeError> {
        match dst {
            ValueSlot::Float(slot) => self.set_float(slot, value),
            _ => self.set_value(dst, RuntimeValue::Float(value)),
        }
    }

    fn set_context_int(&mut self, dst: ValueSlot, value: i32) -> Result<(), RuntimeError> {
        match dst {
            ValueSlot::Int(slot) => self.set_int(slot, value),
            ValueSlot::Float(slot) => self.set_float(slot, value as f32),
            _ => self.set_value(dst, RuntimeValue::Int(value)),
        }
    }

    fn index_value(
        &self,
        target: &RuntimeValue,
        index: &RuntimeValue,
    ) -> Result<RuntimeValue, RuntimeError> {
        match target {
            RuntimeValue::ArraySlot(_)
            | RuntimeValue::ParameterArray(_)
            | RuntimeValue::Array(_) => {
                let index = usize::try_from(to_int_runtime(index, self.params)?)
                    .map_err(|_| RuntimeError::new("array index cannot be negative"))?;
                array_item(
                    target,
                    index,
                    self.workspace.arrays.as_deref(),
                    self.params.arrays.as_deref(),
                )
            }
            RuntimeValue::TargetItems(items) => {
                let index = usize::try_from(to_int_runtime(index, self.params)?)
                    .map_err(|_| RuntimeError::new("target item index cannot be negative"))?;
                let value = items
                    .groups
                    .get(index)
                    .cloned()
                    .ok_or_else(|| RuntimeError::new("target item index out of bounds"))?;
                Ok(RuntimeValue::TargetItem(value))
            }
            RuntimeValue::Curve(curve) => {
                let position = to_float_runtime(index, self.params)?;
                Ok(RuntimeValue::Float(sample_curve(curve, position)))
            }
            RuntimeValue::PreparedCurve(curve) => {
                let position = to_float_runtime(index, self.params)?;
                Ok(RuntimeValue::Float(sample_prepared_curve(curve, position)?))
            }
            RuntimeValue::Gradient(gradient) => {
                let position = to_float_runtime(index, self.params)?;
                Ok(RuntimeValue::Color(sample_gradient(gradient, position)?))
            }
            _ => Err(RuntimeError::new(
                "index target is not an array, curve, or gradient",
            )),
        }
    }

    fn prepared_gradient_param(&self, param: usize) -> Result<&Gradient, RuntimeError> {
        match self.params.values.get(param) {
            Some(BoundParamValue::Gradient(gradient)) => Ok(gradient),
            Some(_) => Err(RuntimeError::new("expected gradient")),
            None => Err(RuntimeError::new("invalid param slot")),
        }
    }

    fn enum_param_equal_const(&self, param: usize, constant: usize) -> Result<bool, RuntimeError> {
        let expected = match self.bytecode.constants.get(constant) {
            Some(Value::Enum(value)) => value,
            Some(_) => return Err(RuntimeError::new("expected enum constant")),
            None => return Err(RuntimeError::new("invalid constant slot")),
        };
        match self.params.values.get(param) {
            Some(BoundParamValue::Enum(value)) => Ok(value == expected),
            Some(_) => Err(RuntimeError::new("expected enum")),
            None => Err(RuntimeError::new("invalid param slot")),
        }
    }

    fn slots_equal(&self, left: ValueSlot, right: ValueSlot) -> Result<bool, RuntimeError> {
        Ok(match (left, right) {
            (ValueSlot::Int(left), ValueSlot::Int(right)) => self.int(left)? == self.int(right)?,
            (ValueSlot::Float(left), ValueSlot::Float(right)) => {
                self.float(left)? == self.float(right)?
            }
            (ValueSlot::Int(left), ValueSlot::Float(right)) => {
                self.int(left)? as f32 == self.float(right)?
            }
            (ValueSlot::Float(left), ValueSlot::Int(right)) => {
                self.float(left)? == self.int(right)? as f32
            }
            (ValueSlot::Bool(left), ValueSlot::Bool(right)) => {
                self.bool(left)? == self.bool(right)?
            }
            (ValueSlot::Color(left), ValueSlot::Color(right)) => {
                self.color(left)? == self.color(right)?
            }
            (ValueSlot::Ref(left), ValueSlot::Ref(right)) => {
                runtime_refs_equal(self.ref_value(left)?, self.ref_value(right)?)
            }
            _ => {
                let left = self.value(left)?;
                let right = self.value(right)?;
                values_equal(&left, &right, self.params)
            }
        })
    }

    fn emit_generated(
        &mut self,
        effect: GeneratedEffectSlot,
        fields: &[(Identifier, ValueSlot)],
    ) -> Result<(), RuntimeError> {
        let mut start_seconds = None;
        let mut duration_seconds = None;
        let mut target = None;
        let mut params = Vec::with_capacity(fields.len());
        for (field, slot) in fields {
            match field.as_str() {
                "start" => start_seconds = Some(self.float_value(*slot)?),
                "duration" => duration_seconds = Some(self.float_value(*slot)?),
                "target" => {
                    target = Some(self.target_item_value(*slot)?);
                }
                _ => {
                    let value = self.value(*slot)?;
                    params.push((
                        field.clone(),
                        runtime_to_value(
                            value,
                            self.workspace.arrays.as_deref(),
                            self.params.arrays.as_deref(),
                        ),
                    ));
                }
            }
        }
        let start_seconds = start_seconds.ok_or_else(|| RuntimeError::new("emit missing start"))?;
        let duration_seconds =
            duration_seconds.ok_or_else(|| RuntimeError::new("emit missing duration"))?;
        let context = generator_context(self.context)?;
        let start_time = sample_time_with_seconds_offset(context.start_time, start_seconds)
            .map_err(|_| RuntimeError::new("emitted effect start is out of range"))?;
        let duration = sample_duration_from_seconds_f32(duration_seconds)
            .map_err(|_| RuntimeError::new("emitted effect duration is out of range"))?;
        if duration.as_ticks() == 0 {
            return Err(RuntimeError::new(
                "emitted effect duration must be positive",
            ));
        }
        let (_, generated) = self
            .generated
            .as_mut()
            .ok_or_else(|| RuntimeError::new("emit is only valid in a generator"))?;
        generated.push(GeneratedEffect {
            definition: effect,
            start_time,
            duration,
            target: target.ok_or_else(|| RuntimeError::new("emit missing target"))?,
            params,
        });
        Ok(())
    }
}

fn resolve_param<'a, P>(param: &ParamDecl, params: P) -> Result<Value, RuntimeError>
where
    P: IntoIterator<Item = (&'a Identifier, &'a Value)>,
{
    if let Some((_, value)) = params.into_iter().find(|(name, _)| **name == param.name) {
        return Ok(value.clone());
    }
    if let Some(default) = &param.default {
        return Ok(default.clone());
    }
    Err(RuntimeError::new(format!(
        "missing required parameter `{}`",
        param.name.as_str()
    )))
}

fn bind_param_value(ty: &Type, value: Value, cache: &mut DslBindCache) -> BoundParamValue {
    match (ty, value) {
        (Type::Float, Value::Int(value)) => BoundParamValue::Float(value as f32),
        (ty, value) => BoundParamValue::from_value(ty, value, cache),
    }
}

fn runtime_to_value(
    value: RuntimeValue,
    arrays: Option<&ArrayStorage>,
    parameter_arrays: Option<&ArrayStorage>,
) -> Value {
    match value {
        RuntimeValue::Void => Value::Void,
        RuntimeValue::Int(value) => Value::Int(value),
        RuntimeValue::Float(value) => Value::Float(value),
        RuntimeValue::Bool(value) => Value::Bool(value),
        RuntimeValue::Color(value) => Value::Color(value),
        RuntimeValue::Marks(value) => Value::Marks(value),
        RuntimeValue::Timeline => Value::Void,
        RuntimeValue::Target(value) => Value::Target(value),
        RuntimeValue::TargetItems(value) => Value::TargetItems(value),
        RuntimeValue::TargetItem(value) => Value::TargetItem(value),
        RuntimeValue::Curve(value) => Value::Curve(value),
        RuntimeValue::PreparedCurve(value) => Value::Curve(value.raw()),
        RuntimeValue::Gradient(value) => Value::Gradient(value),
        RuntimeValue::Array(value) => Value::Array(value),
        RuntimeValue::ArraySlot(index) => Value::Array(
            arrays
                .expect("prepared array storage")
                .items(index)
                .iter()
                .map(|value| runtime_to_value(clone_runtime(value), arrays, parameter_arrays))
                .collect::<Vec<_>>()
                .into(),
        ),
        RuntimeValue::ParameterArray(index) => Value::Array(
            parameter_arrays
                .expect("prepared parameter array storage")
                .items(index)
                .iter()
                .map(|value| {
                    runtime_to_value(parameter_array_value(value), arrays, parameter_arrays)
                })
                .collect::<Vec<_>>()
                .into(),
        ),
        RuntimeValue::Enum(value) => Value::Enum(value),
    }
}

fn member_value(
    target: &RuntimeValue,
    member: &super::bytecode::TargetMember,
) -> Result<RuntimeValue, RuntimeError> {
    let RuntimeValue::TargetItem(item) = target else {
        return Err(RuntimeError::new("member access requires TargetItem"));
    };
    let Some(pixel) = item.pixels.first() else {
        return Err(RuntimeError::new("empty TargetItem has no fields"));
    };
    Ok(match member {
        super::bytecode::TargetMember::FixtureIndex => RuntimeValue::Int(pixel.fixture_index),
        super::bytecode::TargetMember::FixturePixelIndex => {
            RuntimeValue::Int(pixel.fixture_pixel_index)
        }
        super::bytecode::TargetMember::PixelIndex => RuntimeValue::Int(pixel.pixel_index),
        super::bytecode::TargetMember::PixelCount => RuntimeValue::Int(pixel.pixel_count),
        super::bytecode::TargetMember::PixelFraction => RuntimeValue::Float(pixel.pixel_fraction),
    })
}

fn target_item_from_groups(groups: &[Arc<TargetItemValue>]) -> Arc<TargetItemValue> {
    if groups.len() == 1 {
        return Arc::clone(&groups[0]);
    }
    let pixels = groups
        .iter()
        .flat_map(|item| item.pixels.iter().copied())
        .collect::<Vec<_>>();
    Arc::new(TargetItemValue {
        pixels: Arc::from(pixels),
    })
}

fn black() -> Color {
    Color {
        red: 0,
        green: 0,
        blue: 0,
    }
}

fn sample_context(context: VmContext<'_>) -> Result<&RunContext, RuntimeError> {
    match context {
        VmContext::Sample(context) => Ok(context),
        VmContext::Generator(_) => Err(RuntimeError::new("sample context is unavailable")),
    }
}

fn generator_context(context: VmContext<'_>) -> Result<&GeneratorContext, RuntimeError> {
    match context {
        VmContext::Generator(context) => Ok(context),
        VmContext::Sample(_) => Err(RuntimeError::new("generator context is unavailable")),
    }
}

fn to_int_runtime(value: &RuntimeValue, params: &BoundParams) -> Result<i32, RuntimeError> {
    let _ = params;
    match value {
        RuntimeValue::Int(value) => Ok(*value),
        RuntimeValue::Float(value) => Ok(*value as i32),
        _ => Err(RuntimeError::new("expected int")),
    }
}

fn to_float_runtime(value: &RuntimeValue, params: &BoundParams) -> Result<f32, RuntimeError> {
    let _ = params;
    match value {
        RuntimeValue::Int(value) => Ok(*value as f32),
        RuntimeValue::Float(value) => Ok(*value),
        _ => Err(RuntimeError::new("expected float")),
    }
}

fn to_curve_runtime<'a>(
    value: &'a RuntimeValue,
    params: &'a BoundParams,
) -> Result<&'a Curve, RuntimeError> {
    let _ = params;
    match value {
        RuntimeValue::Curve(curve) => Ok(curve),
        RuntimeValue::PreparedCurve(curve) => Ok(&curve.raw),
        _ => Err(RuntimeError::new("expected curve")),
    }
}

fn to_gradient_runtime<'a>(
    value: &'a RuntimeValue,
    params: &'a BoundParams,
) -> Result<&'a Gradient, RuntimeError> {
    let _ = params;
    match value {
        RuntimeValue::Gradient(gradient) => Ok(gradient),
        _ => Err(RuntimeError::new("expected gradient")),
    }
}

fn target_items(value: &RuntimeValue) -> Result<&TargetItemsValue, RuntimeError> {
    match value {
        RuntimeValue::TargetItems(items) => Ok(items),
        _ => Err(RuntimeError::new("expected TargetItems")),
    }
}

fn for_each_target_group(
    value: &RuntimeValue,
    mut visit: impl FnMut(&Arc<TargetItemValue>),
) -> Result<(), RuntimeError> {
    match value {
        RuntimeValue::Target(target) => {
            for group in &target.groups {
                visit(group);
            }
            Ok(())
        }
        RuntimeValue::TargetItems(items) => {
            for group in &items.groups {
                visit(group);
            }
            Ok(())
        }
        RuntimeValue::TargetItem(item) => {
            visit(item);
            Ok(())
        }
        _ => Err(RuntimeError::new("expected target")),
    }
}

fn fixtures(value: &RuntimeValue) -> Result<TargetItemsValue, RuntimeError> {
    let mut raw_groups: Vec<Vec<TargetPixelValue>> = Vec::new();
    for_each_target_pixel(value, |pixel| {
        if raw_groups
            .last()
            .and_then(|group| group.first())
            .is_some_and(|first| first.fixture_index == pixel.fixture_index)
        {
            if let Some(group) = raw_groups.last_mut() {
                group.push(pixel);
            }
        } else {
            raw_groups.push(vec![pixel]);
        }
    })?;
    let groups = raw_groups
        .into_iter()
        .map(|pixels| {
            Arc::new(TargetItemValue {
                pixels: Arc::from(pixels),
            })
        })
        .collect();
    Ok(TargetItemsValue { groups })
}

fn pixels(value: &RuntimeValue) -> Result<TargetItemsValue, RuntimeError> {
    let mut groups = Vec::new();
    for_each_target_pixel(value, |pixel| {
        groups.push(Arc::new(TargetItemValue {
            pixels: Arc::from([pixel]),
        }));
    })?;
    Ok(TargetItemsValue { groups })
}

fn sections(value: &RuntimeValue, width: f32) -> Result<TargetItemsValue, RuntimeError> {
    let width = libm::floorf(width.max(1.0)) as i32;
    let mut raw_groups: Vec<Vec<TargetPixelValue>> = Vec::new();
    for_each_target_pixel(value, |pixel| {
        if raw_groups
            .last()
            .and_then(|group| group.first())
            .is_some_and(|first| {
                first.fixture_index == pixel.fixture_index
                    && first.fixture_pixel_index / width == pixel.fixture_pixel_index / width
            })
        {
            if let Some(group) = raw_groups.last_mut() {
                group.push(pixel);
            }
        } else {
            raw_groups.push(vec![pixel]);
        }
    })?;
    let groups = raw_groups
        .into_iter()
        .map(|pixels| {
            Arc::new(TargetItemValue {
                pixels: Arc::from(pixels),
            })
        })
        .collect();
    Ok(TargetItemsValue { groups })
}

fn for_each_target_pixel(
    value: &RuntimeValue,
    mut visit: impl FnMut(TargetPixelValue),
) -> Result<(), RuntimeError> {
    for_each_target_group(value, |item| {
        for pixel in item.pixels.iter().copied() {
            visit(pixel);
        }
    })
}

fn values_equal(left: &RuntimeValue, right: &RuntimeValue, params: &BoundParams) -> bool {
    let _ = params;
    runtime_refs_equal(left, right)
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

fn sample_prepared_curve(curve: &PreparedCurve, position: f32) -> Result<f32, RuntimeError> {
    Ok(sample_curve(&curve.raw, position))
}

pub(crate) fn prepared_curve_crossing(
    crossings: &PreparedCurveCrossings,
    value: f32,
    fallback: f32,
) -> Result<f32, RuntimeError> {
    match crossings.segments() {
        [] => return Ok(fallback),
        [segment] => return Ok(crossing_at(segment, value).unwrap_or(fallback)),
        _ => {}
    }
    Ok(match crossings {
        PreparedCurveCrossings::Increasing(segments) => {
            let index = segments.partition_point(|segment| segment.max_value < value);
            segments
                .get(index)
                .and_then(|segment| crossing_at(segment, value))
                .unwrap_or(fallback)
        }
        PreparedCurveCrossings::Decreasing(segments) => {
            let index = segments.partition_point(|segment| segment.min_value > value);
            segments
                .get(index)
                .and_then(|segment| crossing_at(segment, value))
                .unwrap_or(fallback)
        }
        PreparedCurveCrossings::Mixed(segments) => segments
            .iter()
            .find_map(|segment| crossing_at(segment, value))
            .unwrap_or(fallback),
    })
}

fn curve_crossing_raw(curve: &Curve, value: f32, fallback: f32) -> f32 {
    crate::sampling::curve_crossing(curve, value, fallback)
}

#[inline(always)]
fn crossing_at(segment: &CrossingSegment, value: f32) -> Option<f32> {
    if value < segment.min_value || value > segment.max_value {
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
            for value in [-0.1, 0.0, 0.1, 0.2, 0.5, 0.8, 1.0, 1.1] {
                let expected = curve_crossing(&curve, value, -7.0);
                let actual = prepared_curve_crossing(&prepared.crossings, value, -7.0).unwrap();
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
            prepared_curve_crossing(&curve.crossings, 0.75, -1.0),
            Ok(0.25)
        );
        assert_eq!(
            prepared_curve_crossing(&curve.crossings, 0.5, -1.0),
            Ok(-1.0)
        );
    }
}

fn sample_curve(curve: &Curve, position: f32) -> f32 {
    crate::sampling::sample_curve(curve, position)
}

fn sample_gradient(gradient: &Gradient, position: f32) -> Result<Color, RuntimeError> {
    crate::sampling::sample_gradient(gradient, position)
        .ok_or_else(|| RuntimeError::new("cannot sample empty gradient"))
}

fn channel(value: f32) -> u8 {
    channel_byte(value * 255.0)
}

fn channel_byte(value: f32) -> u8 {
    (value.clamp(0.0, 255.0) + 0.5) as u8
}

fn mark_at_from(marks: &Marks, index: i32, fallback: f32) -> Result<f32, RuntimeError> {
    let index =
        usize::try_from(index).map_err(|_| RuntimeError::new("mark index cannot be negative"))?;
    Ok(marks
        .marks
        .get(index)
        .map(|mark| sample_duration_seconds_f32(*mark))
        .unwrap_or(fallback))
}

fn prev_index(marks: &Marks, seconds: f32) -> Result<i32, RuntimeError> {
    let mut previous = -1;
    for (index, mark) in marks.marks.iter().enumerate() {
        if sample_duration_seconds_f32(*mark) <= seconds {
            previous = i32::try_from(index)
                .map_err(|_| RuntimeError::new("mark index exceeds int range"))?;
        }
    }
    Ok(previous)
}

fn next_index(marks: &Marks, seconds: f32) -> Result<i32, RuntimeError> {
    for (index, mark) in marks.marks.iter().enumerate() {
        if sample_duration_seconds_f32(*mark) > seconds {
            return i32::try_from(index)
                .map_err(|_| RuntimeError::new("mark index exceeds int range"));
        }
    }
    Ok(-1)
}

fn elapsed(marks: &Marks, seconds: f32) -> Result<f32, RuntimeError> {
    let previous = prev_index(marks, seconds)?;
    if previous < 0 {
        return Ok(seconds);
    }
    Ok(seconds - mark_at_from(marks, previous, 0.0)?)
}

fn phase(marks: &Marks, seconds: f32, duration: f32) -> Result<f32, RuntimeError> {
    let previous = prev_index(marks, seconds)?;
    let next = next_index(marks, seconds)?;
    let start = if previous >= 0 {
        mark_at_from(marks, previous, 0.0)?
    } else {
        0.0
    };
    let end = if next >= 0 {
        mark_at_from(marks, next, duration)?
    } else {
        duration
    };
    Ok(((seconds - start) / (end - start).max(0.000000001)).clamp(0.0, 1.0))
}
