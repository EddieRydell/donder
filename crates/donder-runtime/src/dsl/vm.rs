mod arrays;
mod context;
mod lanes;
pub(super) use context::NoSignals;
use context::{ReadContext, SampleSignal};
use core::convert::Infallible;
use lanes::{Flow, Mask, dispatch};
mod automation;
mod parameters;
use arrays::ArrayRegister;
pub(crate) use automation::AutomationPlan;

use parameters::{
    CurveRegister, GradientRegister, MarksRegister, ParameterAddress, ParameterValues,
};

use super::bytecode::{
    ArraySlot, BoolSlot, BytecodeProgram, ColorBinary, ColorComponent, ColorSlot, CompareOp,
    ContextRead, CurveSlot, EnumSlot, EnumSlotType, FloatBinary, FloatSlot, FloatUnary,
    GradientSlot, Instruction, IntSlot, MarkOp, MarksSlot, NumberSlot, ParameterKind, SignalPixel,
    SlotLayout, ValueSlot,
};
use super::types::{Identifier, Type, Value};
use crate::sampling::{
    add_colors, clamp_float, color_hue, color_intensity, color_saturation, invert_color,
    max_colors, mix_colors, multiply_colors, scale_color,
};
use crate::values::{
    Color, Curve, Gradient, Marks, SampleDuration, SampleTime, sample_duration_seconds_f32,
};
use alloc::boxed::Box;
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

#[derive(Clone, Copy)]
pub(crate) struct LaneContext<'a> {
    pub context: RunContext,
    pub spatial: SpatialContext,
    pub sections: crate::sections::SectionContext<'a>,
}

#[cfg(test)]
const TEST_SPATIAL_CONTEXT: SpatialContext = SpatialContext {
    position: [0.0; 2],
    min: [0.0; 2],
    max: [0.0; 2],
};

/// Samples an immutable signal. Identical input/time/pixel
/// queries must produce the same result; compilation and evaluation may reuse it.
pub(crate) trait SignalSampler<E = RuntimeError> {
    fn sample_signal(
        &mut self,
        input: usize,
        sample_time: SampleTime,
        pixel: SignalPixel<i32>,
        frame_cache: Option<usize>,
    ) -> Result<Color, E>;

    fn sample_signal_block(
        &mut self,
        input: usize,
        sample_time: SampleTime,
        pixel: SignalPixel<i32>,
        output: &mut [Color],
    ) -> Result<(), E> {
        // Only the prepared graph invokes color blocks. Scalar adapters keep
        // their original error ordering and never receive a multi-lane query.
        assert_eq!(output.len(), 1);
        output[0] = self.sample_signal(input, sample_time, pixel, None)?;
        Ok(())
    }
}

pub(crate) const COLOR_BLOCK_WIDTH: usize = 32;

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

    /// Materialize all slots without an out-of-range lookup.
    #[cfg(test)]
    pub(crate) fn iter_values(&self) -> impl Iterator<Item = Value> + '_ {
        self.values
            .iter()
            .map(|value| runtime_to_value(value.to_runtime(), &ArrayStorage::default()))
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

    #[cfg(test)]
    pub(crate) fn sample_gradient(
        &self,
        index: usize,
        position: f32,
    ) -> Result<Color, RuntimeError> {
        self.values
            .gradient(index)
            .map(|value| sample_gradient(value.get(), position))
            .ok_or_else(|| RuntimeError::new("expected gradient parameter"))
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

#[derive(Debug, Default)]
pub(crate) struct VmWorkspace {
    registers: VmRegisters,
    arrays: ArrayStorage,
    // Collection length is not a DSL int. Only the visible loop index wraps.
    loop_remaining: Vec<usize>,
    target_count: i32,
    target_bounds: [u32; 4],
    color_lanes: usize,
    numeric_lanes: usize,
    numeric_uniform_target: bool,
}

impl VmWorkspace {
    pub(super) fn sample_entry(
        &mut self,
        reuse_uniform: bool,
        target_entry: usize,
        pixel_entry: usize,
        context: &RunContext,
        spatial: &SpatialContext,
    ) -> usize {
        let reuse_uniform = reuse_uniform && self.color_lanes <= 1;
        if target_entry == pixel_entry {
            return if reuse_uniform { pixel_entry } else { 0 };
        }
        let bounds = target_bounds_bits(spatial);
        let same_target = self.target_count == context.pixel_count && self.target_bounds == bounds;
        self.target_count = context.pixel_count;
        self.target_bounds = bounds;
        if !reuse_uniform {
            0
        } else if same_target {
            pixel_entry
        } else {
            target_entry
        }
    }

    #[cfg(test)]
    pub(crate) fn for_program<C, S, A>(program: &BytecodeProgram<C, S, A>) -> Self {
        let mut workspace = Self::default();
        workspace.reserve(program);
        workspace
    }

    pub(crate) fn storage_estimate(
        registers: [usize; 9],
        capacity: usize,
        width: usize,
        loop_count: usize,
    ) -> Option<usize> {
        let sizes = [
            size_of::<i32>(),
            size_of::<f32>(),
            size_of::<bool>(),
            size_of::<Color>(),
            size_of::<ArrayRegister>(),
            size_of::<MarksRegister>(),
            size_of::<CurveRegister>(),
            size_of::<GradientRegister>(),
            size_of::<Identifier>(),
        ];
        let mut bytes = size_of::<Self>();
        for (count, size) in registers.into_iter().zip(sizes) {
            bytes = bytes.checked_add(count.checked_mul(size)?)?;
        }
        bytes = bytes.checked_add(loop_count.checked_mul(size_of::<usize>())?)?;
        if capacity != 0 {
            bytes = bytes
                .checked_add(capacity.checked_mul(3 * size_of::<usize>())?)?
                .checked_add(
                    capacity
                        .checked_mul(width)?
                        .checked_mul(size_of::<RuntimeValue>())?,
                )?;
        }
        Some(bytes)
    }

    pub(crate) fn reserve<C, S, A>(&mut self, bytecode: &BytecodeProgram<C, S, A>) {
        self.registers.reserve(bytecode.layout);
        self.reserve_arrays(bytecode);
        self.loop_remaining.resize(
            self.loop_remaining.len().max(bytecode.loop_count as usize),
            0,
        );
    }

    pub(crate) fn reserve_color_block(&mut self, colors: u32) {
        reserve(
            &mut self.registers.colors,
            colors as usize * COLOR_BLOCK_WIDTH,
        );
    }

    pub(crate) fn reserve_numeric_block<C, S, A>(&mut self, bytecode: &BytecodeProgram<C, S, A>) {
        let layout = bytecode.layout;
        reserve(
            &mut self.registers.ints,
            layout.ints as usize * COLOR_BLOCK_WIDTH,
        );
        reserve(
            &mut self.registers.floats,
            layout.floats as usize * COLOR_BLOCK_WIDTH,
        );
        reserve(
            &mut self.registers.bools,
            layout.bools as usize * COLOR_BLOCK_WIDTH,
        );
        self.reserve_color_block(layout.colors);
        self.loop_remaining.resize(
            self.loop_remaining
                .len()
                .max(bytecode.loop_count as usize * COLOR_BLOCK_WIDTH),
            0,
        );
    }

    fn reserve_arrays<C, S, A>(&mut self, bytecode: &BytecodeProgram<C, S, A>) {
        if bytecode.array_capacity == 0 {
            return;
        }
        let (capacity, width) = (self.arrays.references.len(), self.arrays.width);
        if capacity < bytecode.array_capacity as usize || width < bytecode.array_width as usize {
            self.arrays = ArrayStorage::new(
                capacity.max(bytecode.array_capacity as usize),
                width.max(bytecode.array_width as usize),
            );
        }
    }
}

fn target_bounds_bits(spatial: &SpatialContext) -> [u32; 4] {
    [
        spatial.min[0].to_bits(),
        spatial.min[1].to_bits(),
        spatial.max[0].to_bits(),
        spatial.max[1].to_bits(),
    ]
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

#[derive(Clone, Debug, Default)]
struct VmRegisters {
    ints: Vec<i32>,
    floats: Vec<f32>,
    bools: Vec<bool>,
    colors: Vec<Color>,
    array_values: Vec<ArrayRegister>,
    enums: Vec<Identifier>,
    marks: Vec<MarksRegister>,
    curves: Vec<CurveRegister>,
    gradients: Vec<GradientRegister>,
}

impl VmRegisters {
    /// Query initialization writes lane zero. Replicate once at the stage
    /// boundary, keeping broadcasts out of every arithmetic register write.
    #[inline(never)]
    fn broadcast_numeric(&mut self) {
        for register in self.ints.as_chunks_mut::<COLOR_BLOCK_WIDTH>().0 {
            let value = register[0];
            register.fill(value);
        }
        for register in self.floats.as_chunks_mut::<COLOR_BLOCK_WIDTH>().0 {
            let value = register[0];
            register.fill(value);
        }
        for register in self.bools.as_chunks_mut::<COLOR_BLOCK_WIDTH>().0 {
            let value = register[0];
            register.fill(value);
        }
        for register in self.colors.as_chunks_mut::<COLOR_BLOCK_WIDTH>().0 {
            let value = register[0];
            register.fill(value);
        }
    }

    fn reserve(&mut self, layout: SlotLayout) {
        reserve(&mut self.ints, layout.ints as usize);
        reserve(&mut self.floats, layout.floats as usize);
        reserve(&mut self.bools, layout.bools as usize);
        reserve(&mut self.colors, layout.colors as usize);
        reserve(&mut self.array_values, layout.arrays as usize);
        reserve(&mut self.enums, layout.enums as usize);
        reserve(&mut self.marks, layout.marks as usize);
        reserve(&mut self.curves, layout.curves as usize);
        reserve(&mut self.gradients, layout.gradients as usize);
    }

    fn prepare<C, S, A>(
        &mut self,
        bytecode: &BytecodeProgram<C, S, A>,
        lanes: usize,
        numeric: usize,
    ) {
        if self.ints.len() == bytecode.layout.ints as usize * numeric
            && self.floats.len() == bytecode.layout.floats as usize * numeric
            && self.bools.len() == bytecode.layout.bools as usize * numeric
            && self.colors.len() == bytecode.layout.colors as usize * lanes
            && self.array_values.len() == bytecode.layout.arrays as usize
            && self.enums.len() == bytecode.layout.enums as usize
            && self.marks.len() == bytecode.layout.marks as usize
            && self.curves.len() == bytecode.layout.curves as usize
            && self.gradients.len() == bytecode.layout.gradients as usize
        {
            return;
        }
        self.resize(bytecode.layout, &bytecode.enum_types, lanes, numeric);
    }

    // Keep shape changes and value initialization out of the hot constructor.
    // The length check above still runs on every independent invocation.
    #[inline(never)]
    fn resize(&mut self, layout: SlotLayout, enums: &[EnumSlotType], lanes: usize, numeric: usize) {
        self.ints.clear();
        self.ints.resize(layout.ints as usize * numeric, 0);
        self.floats.clear();
        self.floats.resize(layout.floats as usize * numeric, 0.0);
        self.bools.clear();
        self.bools.resize(layout.bools as usize * numeric, false);
        self.colors.clear();
        self.colors.resize(layout.colors as usize * lanes, black());
        self.enums.clear();
        self.enums
            .extend(enums.iter().map(|ty| ty.initial().clone()));
        self.array_values.clear();
        self.array_values
            .resize(layout.arrays as usize, ArrayRegister::Empty);
        self.curves.clear();
        self.curves
            .resize(layout.curves as usize, CurveRegister::Empty);
        self.gradients.clear();
        self.gradients
            .resize(layout.gradients as usize, GradientRegister::Empty);
        self.marks.clear();
        self.marks
            .resize(layout.marks as usize, MarksRegister::Empty);
    }
}

fn reserve<T>(values: &mut Vec<T>, capacity: usize) {
    if values.capacity() < capacity {
        values.reserve_exact(capacity - values.len());
    }
}

#[cfg(test)]
mod workspace_capacity_tests {
    use super::*;

    #[test]
    fn scalar_constants_preserve_values_without_a_constant_pool() {
        let floats = [
            0.0f32.to_bits(),
            (-0.0f32).to_bits(),
            f32::INFINITY.to_bits(),
            f32::NEG_INFINITY.to_bits(),
            f32::MIN_POSITIVE.to_bits(),
            1,           // Smallest positive subnormal.
            0x7fc0_1234, // Quiet NaN payload.
            0x7f80_1234, // Signaling NaN payload; a load must not do arithmetic.
            0xffc0_1234,
        ];
        let ints = [i32::MIN, i32::MAX, 16_777_217];
        let color = Color {
            red: 1,
            green: 127,
            blue: 255,
        };
        let mut instructions: Vec<_> = floats
            .iter()
            .enumerate()
            .map(|(index, bits)| Instruction::LoadFloatConst {
                dst: FloatSlot(index as u32),
                bits: *bits,
            })
            .collect();
        instructions.extend(ints.iter().enumerate().map(|(index, value)| {
            Instruction::LoadIntConst {
                dst: IntSlot(index as u32),
                value: *value,
            }
        }));
        instructions.extend([
            Instruction::LoadBoolConst {
                dst: BoolSlot(0),
                value: false,
            },
            Instruction::LoadBoolConst {
                dst: BoolSlot(1),
                value: true,
            },
            Instruction::LoadColorConst {
                dst: ColorSlot(0),
                value: color,
            },
            Instruction::ReturnColor(ColorSlot(0)),
        ]);
        let archived = rkyv::to_bytes::<rkyv::rancor::Failure>(&instructions).unwrap();
        let restored =
            rkyv::from_bytes::<Vec<Instruction>, rkyv::rancor::Failure>(&archived).unwrap();
        assert_eq!(instructions, restored);
        let mut program = BytecodeProgram {
            instructions: restored.into(),
            curves: Box::new([]),
            gradients: Box::new([]),
            enums: Box::new([]),
            enum_types: Box::new([]),
            array_constants: Box::new([]),
            value_operands: Box::new([]),
            array_types: Box::new([]),
            layout: SlotLayout {
                ints: ints.len() as u32,
                floats: floats.len() as u32,
                bools: 2,
                colors: 1,
                ..SlotLayout::default()
            },
            uses_pixel_context: false,
            pixel_entry: 0,
            array_capacity: 0,
            array_width: 0,
            loop_count: 0,
        };
        assert!(program.has_valid_structure());
        let context = RunContext {
            progress: 0.0,
            time: SampleDuration::from_ticks(0),
            duration: SampleDuration::from_ticks(1),
            pixel_index: 0,
            pixel_count: 1,
            pixel_fraction: 0.0,
        };
        let params = BoundParams::default();
        let mut workspace = VmWorkspace::for_program(&program);
        let admitted = super::super::SampleProgram::admit(program.clone(), Box::new([])).unwrap();
        // Reusing the workspace must preserve the same values too.
        for _ in 0..2 {
            let mut signals = NoSignals;
            let mut vm = Vm::new(
                admitted.bytecode(),
                &params,
                &context,
                &TEST_SPATIAL_CONTEXT,
                &mut workspace,
                &mut signals as &mut dyn SignalSampler<Infallible>,
                0,
            );
            let result = vm.run().unwrap();
            assert_eq!(vm.color(result), color);
            assert_eq!(vm.workspace.registers.ints, ints);
            assert_eq!(vm.workspace.registers.bools, [false, true]);
            for (actual, expected) in vm.workspace.registers.floats.iter().zip(floats) {
                assert_eq!(actual.to_bits(), expected);
            }
        }
        // Scalar payload kinds cannot be mismatched; register bounds still need admission.
        for invalid in [
            Instruction::LoadIntConst {
                dst: IntSlot(program.layout.ints),
                value: 0,
            },
            Instruction::LoadFloatConst {
                dst: FloatSlot(program.layout.floats),
                bits: 0,
            },
            Instruction::LoadBoolConst {
                dst: BoolSlot(program.layout.bools),
                value: false,
            },
            Instruction::LoadColorConst {
                dst: ColorSlot(program.layout.colors),
                value: color,
            },
        ] {
            program.instructions[0] = invalid;
            assert!(!program.has_valid_structure());
        }
    }

    #[test]
    fn context_reads_preserve_integer_precision_and_float_values() {
        let reads = [
            (NumberSlot::Int(IntSlot(0)), ContextRead::PixelIndex),
            (NumberSlot::Int(IntSlot(1)), ContextRead::PixelCount),
            (NumberSlot::Float(FloatSlot(0)), ContextRead::PixelIndex),
            (NumberSlot::Float(FloatSlot(1)), ContextRead::PixelCount),
            (NumberSlot::Float(FloatSlot(2)), ContextRead::Progress),
            (NumberSlot::Float(FloatSlot(3)), ContextRead::Seconds),
            (NumberSlot::Float(FloatSlot(4)), ContextRead::Duration),
            (NumberSlot::Float(FloatSlot(5)), ContextRead::PixelFraction),
            (NumberSlot::Float(FloatSlot(6)), ContextRead::PixelX),
            (NumberSlot::Float(FloatSlot(7)), ContextRead::PixelY),
            (NumberSlot::Float(FloatSlot(8)), ContextRead::TargetMinX),
            (NumberSlot::Float(FloatSlot(9)), ContextRead::TargetMinY),
            (NumberSlot::Float(FloatSlot(10)), ContextRead::TargetMaxX),
            (NumberSlot::Float(FloatSlot(11)), ContextRead::TargetMaxY),
        ];
        let mut instructions: Vec<_> = reads
            .into_iter()
            .map(|(dst, read)| Instruction::ContextRead { dst, read })
            .collect();
        instructions.push(Instruction::ReturnColor(ColorSlot(0)));
        let program = BytecodeProgram {
            instructions: instructions.into(),
            curves: Box::new([]),
            gradients: Box::new([]),
            enums: Box::new([]),
            enum_types: Box::new([]),
            array_constants: Box::new([]),
            value_operands: Box::new([]),
            array_types: Box::new([]),
            layout: SlotLayout {
                ints: 2,
                floats: 12,
                colors: 1,
                ..SlotLayout::default()
            },
            uses_pixel_context: true,
            pixel_entry: 0,
            array_capacity: 0,
            array_width: 0,
            loop_count: 0,
        };
        assert!(program.has_valid_structure());
        let context = RunContext {
            progress: f32::NAN,
            time: SampleDuration::from_ticks(1_250_000),
            duration: SampleDuration::from_ticks(4_000_000),
            pixel_index: 16_777_217,
            pixel_count: i32::MIN,
            pixel_fraction: f32::INFINITY,
        };
        let spatial = SpatialContext {
            position: [0.25, -0.75],
            min: [-1.0, -2.0],
            max: [1.0, 2.0],
        };
        let program = super::super::SampleProgram::admit(program, Box::new([])).unwrap();
        let accepted = program.bind(Vec::new()).unwrap();
        let params = BoundParams::from_validated(&accepted, &mut DslBindCache::default());
        let mut workspace = VmWorkspace::for_program(program.bytecode());
        super::super::SampleProgramExt::sample(
            &program,
            &params,
            &context,
            &spatial,
            crate::sections::SectionContext::Single {
                index: context.pixel_index,
                count: context.pixel_count,
            },
            &mut workspace,
            false,
        );
        assert_eq!(
            workspace.registers.ints,
            [context.pixel_index, context.pixel_count]
        );
        let expected = [
            context.pixel_index as f32,
            context.pixel_count as f32,
            context.progress,
            1.25,
            4.0,
            context.pixel_fraction,
            spatial.position[0],
            spatial.position[1],
            spatial.min[0],
            spatial.min[1],
            spatial.max[0],
            spatial.max[1],
        ];
        for (actual, expected) in workspace.registers.floats.iter().zip(expected) {
            assert_eq!(actual.to_bits(), expected.to_bits());
        }
    }

    #[test]
    fn reserve_grows_from_capacity_even_when_most_slots_are_unused() {
        let mut values = alloc::vec::Vec::<u32>::with_capacity(16);
        values.push(1);
        let required = values.capacity() + 1;
        super::reserve(&mut values, required);
        assert!(values.capacity() >= required);
        assert_eq!(values.as_slice(), &[1]);
    }

    #[test]
    fn collection_countdown_does_not_narrow_to_the_visible_integer_index() {
        let program = BytecodeProgram {
            instructions: Box::new([
                Instruction::LoadMarksConst {
                    dst: MarksSlot(0),
                    value: Arc::new(Marks {
                        marks: vec![SampleDuration::from_ticks(0)],
                    }),
                },
                Instruction::LoopMarksStart {
                    id: 0,
                    marks: MarksSlot(0),
                    end: 4,
                },
                Instruction::JumpIfTrue {
                    condition: BoolSlot(0),
                    target: 5,
                },
                Instruction::IntAdd {
                    dst: IntSlot(0),
                    left: IntSlot(0),
                    right: IntSlot(1),
                },
                Instruction::LoopEnd { id: 0, start: 2 },
                Instruction::ReturnColor(ColorSlot(0)),
            ]),
            curves: Box::new([]),
            gradients: Box::new([]),
            enums: Box::new([]),
            enum_types: Box::new([]),
            array_constants: Box::new([]),
            value_operands: Box::new([]),
            array_types: Box::new([]),
            layout: SlotLayout {
                ints: 2,
                bools: 1,
                colors: 1,
                arrays: 0,
                enums: 0,
                floats: 0,
                marks: 1,
                curves: 0,
                gradients: 0,
            },
            uses_pixel_context: false,
            pixel_entry: 0,
            array_capacity: 0,
            array_width: 0,
            loop_count: 1,
        };
        let params = BoundParams::default();
        let context = RunContext {
            progress: 0.0,
            time: SampleDuration::from_ticks(0),
            duration: SampleDuration::from_ticks(1),
            pixel_index: 0,
            pixel_count: 0,
            pixel_fraction: 0.0,
        };
        // Resume one loop tail with a native-sized countdown. The next body
        // returns immediately, so this needs neither billions of marks nor
        // billions of iterations to test the boundary.
        let admitted = super::super::SampleProgram::admit(program.clone(), Box::new([])).unwrap();
        for remaining in [0, 1, 2, i32::MAX as usize + 2, usize::MAX] {
            let mut workspace = VmWorkspace::default();
            workspace.registers.prepare(&program, 1, 1);
            workspace.registers.ints.copy_from_slice(&[i32::MAX, 1]);
            workspace.registers.bools[0] = true;
            workspace.loop_remaining.push(remaining);
            let mut signals = NoSignals;
            let mut vm = Vm::new(
                admitted.bytecode(),
                &params,
                &context,
                &TEST_SPATIAL_CONTEXT,
                &mut workspace,
                &mut signals as &mut dyn SignalSampler<Infallible>,
                3,
            );
            let result = vm.run().unwrap();
            assert_eq!(vm.color(result), Color::BLACK);
            assert_eq!(vm.workspace.registers.ints[0], i32::MIN);
            assert_eq!(vm.workspace.loop_remaining[0], remaining.saturating_sub(1));
        }
    }
}

pub(super) fn evaluate_sample(
    program: &BytecodeProgram<ContextRead, super::SignalAccess, ColorSlot>,
    params: &BoundParams,
    context: &RunContext,
    spatial: &SpatialContext,
    sections: crate::sections::SectionContext<'_>,
    workspace: &mut VmWorkspace,
    entry: usize,
) -> Color {
    workspace.color_lanes = 1;
    workspace.numeric_lanes = 1;
    let mut signals = NoSignals;
    let sampler: &mut dyn SignalSampler<Infallible> = &mut signals;
    let mut vm = Vm::new(program, params, context, spatial, workspace, sampler, entry);
    vm.sections = sections;
    match vm.run() {
        Ok(slot) => vm.color(slot),
        Err(never) => match never {},
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn evaluate_operator<E>(
    program: &BytecodeProgram<ContextRead, super::SignalAccess, ColorSlot>,
    params: &BoundParams,
    context: &RunContext,
    spatial: &SpatialContext,
    sections: crate::sections::SectionContext<'_>,
    sampler: &mut dyn SignalSampler<E>,
    workspace: &mut VmWorkspace,
    entry: usize,
) -> Result<Color, E> {
    workspace.color_lanes = 1;
    workspace.numeric_lanes = 1;
    let mut vm = Vm::new(program, params, context, spatial, workspace, sampler, entry);
    vm.sections = sections;
    vm.run().map(|slot| vm.color(slot))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn evaluate_operator_block(
    program: &BytecodeProgram<ContextRead, super::SignalAccess, ColorSlot>,
    params: &BoundParams,
    context: &RunContext,
    spatial: &SpatialContext,
    sampler: &mut dyn SignalSampler<Infallible>,
    workspace: &mut VmWorkspace,
    output: &mut [Color],
    reuse_uniform: bool,
) {
    assert!(!output.is_empty() && output.len() <= COLOR_BLOCK_WIDTH);
    // A tail or intervening scalar query changes color-register strides. Run
    // initialization again in that case; otherwise retain the proven prefix.
    let entry =
        if reuse_uniform && workspace.color_lanes == output.len() && workspace.numeric_lanes == 1 {
            program.pixel_entry as usize
        } else {
            0
        };
    workspace.color_lanes = output.len();
    workspace.numeric_lanes = 1;
    let mut vm = Vm::new(program, params, context, spatial, workspace, sampler, entry);
    let slot = match vm.run() {
        Ok(slot) => slot,
        Err(never) => match never {},
    };
    let start = slot.0 as usize * output.len();
    output.copy_from_slice(&vm.workspace.registers.colors[start..start + output.len()]);
}

#[allow(clippy::too_many_arguments)]
pub(super) fn evaluate_numeric_block(
    program: &BytecodeProgram<ContextRead, super::SignalAccess, ColorSlot>,
    params: &BoundParams,
    context: &RunContext,
    spatial: &SpatialContext,
    contexts: Option<&[LaneContext<'_>]>,
    sampler: &mut dyn SignalSampler<Infallible>,
    workspace: &mut VmWorkspace,
    output: &mut [Color],
    reuse_uniform: bool,
    target_entry: usize,
) {
    assert!(!output.is_empty() && output.len() <= COLOR_BLOCK_WIDTH);
    assert!(contexts.is_none_or(|contexts| contexts.len() == output.len()));
    let reuse_uniform = reuse_uniform && workspace.numeric_lanes == COLOR_BLOCK_WIDTH;
    let mut entry = if reuse_uniform { target_entry } else { 0 };
    let mut broadcast_end = target_entry;
    let pixel_entry = program.pixel_entry as usize;
    if target_entry != pixel_entry {
        let bounds = target_bounds_bits(spatial);
        let uniform_target = contexts.is_none_or(|contexts| {
            contexts.iter().all(|lane| {
                lane.context.pixel_count == context.pixel_count
                    && target_bounds_bits(&lane.spatial) == bounds
            })
        });
        if uniform_target {
            // Target-invariant arithmetic still runs once, including division.
            // Reuse is valid across tails because broadcasting initializes all
            // physical lanes, not only those active in the current block.
            if reuse_uniform
                && workspace.numeric_uniform_target
                && workspace.target_count == context.pixel_count
                && workspace.target_bounds == bounds
            {
                entry = pixel_entry;
            }
            broadcast_end = pixel_entry;
            workspace.target_count = context.pixel_count;
            workspace.target_bounds = bounds;
        }
        workspace.numeric_uniform_target = uniform_target;
    } else {
        workspace.numeric_uniform_target = false;
    }
    workspace.color_lanes = COLOR_BLOCK_WIDTH;
    workspace.numeric_lanes = COLOR_BLOCK_WIDTH;
    let width = output.len();
    let mut vm = Vm::new(program, params, context, spatial, workspace, sampler, entry);
    vm.lanes = Some(Flow::new(width));
    vm.broadcast_end = broadcast_end;
    vm.broadcast = entry < broadcast_end;
    if !vm.broadcast {
        vm.active = Mask::full(width);
    }
    vm.lane_contexts = contexts;
    vm.output = Some(output);
    match vm.run() {
        Ok(_) => {}
        Err(never) => match never {},
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

struct Vm<'a, C: ReadContext, S, A, P> {
    bytecode: &'a BytecodeProgram<C, S, A>,
    params: &'a BoundParams,
    context: &'a RunContext,
    spatial: &'a C::Spatial,
    sections: crate::sections::SectionContext<'a>,
    workspace: &'a mut VmWorkspace,
    entry: usize,
    lane: usize,
    lanes: Option<Flow>,
    active: Mask,
    broadcast: bool,
    broadcast_end: usize,
    lane_contexts: Option<&'a [LaneContext<'a>]>,
    output: Option<&'a mut [Color]>,
    signal_sampler: P,
}

impl<C: ReadContext, S, A, P> Drop for Vm<'_, C, S, A, P> {
    fn drop(&mut self) {
        // Local values must not keep parameter resources shared between invocations:
        // the next automation update needs exclusive access to its prepared curves.
        // Preserve register lengths/capacities so the next invocation reuses storage.
        // Assign directly: Clone-based slice fill produces unnecessary variant dispatch.
        for value in &mut self.workspace.registers.array_values {
            if let ArrayRegister::Local(index) = core::mem::take(value) {
                self.workspace
                    .arrays
                    .release(RuntimeValue::ArraySlot(index));
            }
        }
        self.workspace.registers.marks.fill(MarksRegister::Empty);
        self.workspace.registers.curves.fill(CurveRegister::Empty);
        self.workspace
            .registers
            .gradients
            .fill(GradientRegister::Empty);
    }
}

impl<'a, C: ReadContext<Spatial = SpatialContext>, S: Copy, A, P: SampleSignal<S>>
    Vm<'a, C, S, A, P>
{
    #[allow(clippy::too_many_arguments)]
    fn new(
        bytecode: &'a BytecodeProgram<C, S, A>,
        params: &'a BoundParams,
        context: &'a RunContext,
        spatial: &'a C::Spatial,
        workspace: &'a mut VmWorkspace,
        signal_sampler: P,
        entry: usize,
    ) -> Self {
        // A nonzero entry resumes a frame's initialized program/workspace.
        // Independent samples and each frame's first pixel always start at zero.
        workspace.color_lanes = workspace.color_lanes.max(1);
        workspace.numeric_lanes = workspace.numeric_lanes.max(1);
        if entry == 0 {
            workspace
                .registers
                .prepare(bytecode, workspace.color_lanes, workspace.numeric_lanes);
            if bytecode.array_capacity != 0 {
                workspace.reserve_arrays(bytecode);
            }
            workspace.loop_remaining.resize(
                workspace
                    .loop_remaining
                    .len()
                    .max(bytecode.loop_count as usize * workspace.numeric_lanes),
                0,
            );
            workspace.loop_remaining[..bytecode.loop_count as usize * workspace.numeric_lanes]
                .fill(0);
        }
        Self {
            bytecode,
            params,
            context,
            spatial,
            workspace,
            entry,
            lane: 0,
            lanes: None,
            active: Mask::FIRST,
            broadcast: false,
            broadcast_end: bytecode.pixel_entry as usize,
            lane_contexts: None,
            output: None,
            signal_sampler,
            sections: crate::sections::SectionContext::Single {
                index: context.pixel_index,
                count: context.pixel_count,
            },
        }
    }

    fn run(&mut self) -> Result<A, P::Error>
    where
        A: Copy + Into<ColorSlot>,
    {
        let code = self.bytecode.instructions.as_ref();
        let mut instructions = &code[self.entry..];
        let mut next_event = if self.broadcast {
            code.len() - self.broadcast_end
        } else {
            usize::MAX
        };
        loop {
            // Scalar instructions compare against an unreachable sentinel.
            // Numeric stage/join work happens only at the recorded boundary.
            if instructions.len() == next_event {
                if self.broadcast {
                    self.workspace.registers.broadcast_numeric();
                    self.broadcast = false;
                }
                if let Some(flow) = &mut self.lanes {
                    flow.merge(code.len() - instructions.len());
                    self.active = flow.active;
                    next_event = flow.next_join_remaining(code.len());
                }
            }
            // Advance the instruction pointer directly. Only taken branches
            // translate an absolute bytecode target back into a slice.
            let instruction = &instructions[0];
            instructions = &instructions[1..];
            dispatch! { self, code, instructions, instruction, next_event;
                Instruction::LoadIntConst { dst, value } => self.set_int(*dst, *value),
                Instruction::LoadFloatConst { dst, bits } => {
                    self.set_float(*dst, f32::from_bits(*bits))
                }
                Instruction::LoadBoolConst { dst, value } => self.set_bool(*dst, *value),
                Instruction::LoadColorConst { dst, value } => self.set_color(*dst, *value),
                @once Instruction::LoadCurveConst { dst, constant } => self.set_curve(
                    *dst,
                    CurveRegister::Raw(Arc::clone(&self.bytecode.curves[*constant])),
                ),
                @once Instruction::LoadGradientConst { dst, constant } => self.set_gradient(
                    *dst,
                    GradientRegister::Shared(Arc::clone(&self.bytecode.gradients[*constant])),
                ),
                @once Instruction::LoadCurveParam { dst, source, .. } => {
                    self.set_curve(*dst, self.params.values.curves[source.0 as usize].clone())
                }
                @once Instruction::LoadGradientParam { dst, source, .. } => self.set_gradient(
                    *dst,
                    self.params.values.gradients[source.0 as usize].clone(),
                ),
                @once Instruction::CurveSample {
                    dst,
                    curve,
                    position,
                } => self.set_float(*dst, self.curve_value(*curve).sample(self.float(*position))),
                @once Instruction::GradientSample {
                    dst,
                    gradient,
                    position,
                } => self.set_color(
                    *dst,
                    sample_gradient(self.gradient_value(*gradient), self.float(*position)),
                ),
                @once Instruction::LoadMarksConst { dst, value } => {
                    self.set_marks(*dst, MarksRegister::Shared(Arc::clone(value)))
                }
                @once Instruction::LoadMarksParam { dst, source, .. } => {
                    self.set_marks(*dst, self.params.values.marks[source.0 as usize].clone())
                }
                @once Instruction::LoadEnumConst { dst, constant } => {
                    self.set_enum(*dst, self.bytecode.enums[*constant].clone());
                }
                @once Instruction::LoadEnumParam { dst, source, .. } => {
                    self.set_enum(*dst, self.params.values.enums[source.0 as usize].clone());
                }
                @once Instruction::LoadArrayConst { dst, constant } => {
                    self.set_array(
                        *dst,
                        ArrayRegister::Shared(Arc::clone(
                            &self.bytecode.array_constants[*constant],
                        )),
                    );
                }
                Instruction::LoadIntParam { dst, source, .. } => {
                    self.set_int(*dst, self.params.values.ints[source.0 as usize]);
                }
                Instruction::LoadFloatParam { dst, source, .. } => {
                    self.set_float(*dst, self.params.values.floats[source.0 as usize]);
                }
                Instruction::LoadBoolParam { dst, source, .. } => {
                    self.set_bool(*dst, self.params.values.bools[source.0 as usize]);
                }
                Instruction::LoadColorParam { dst, source, .. } => {
                    self.set_color(*dst, self.params.values.colors[source.0 as usize]);
                }
                @once Instruction::LoadArrayParam { dst, source, .. } => {
                    self.set_array(
                        *dst,
                        self.params.values.array_values[source.0 as usize].register(),
                    );
                }
                Instruction::Move { dst, src } => {
                    self.copy_slot(*dst, *src);
                }
                @once Instruction::MakeArray { dst, items } => {
                    let items = &self.bytecode.value_operands[items.range()];
                    let index = self.workspace.arrays.allocate(items.len());
                    for (offset, item) in items.iter().enumerate() {
                        let value = self.value(*item);
                        let arrays = &mut self.workspace.arrays;
                        arrays.retain(&value);
                        arrays.values[index * arrays.width + offset] = value;
                    }
                    self.set_array(*dst, ArrayRegister::Local(index));
                    self.workspace
                        .arrays
                        .release(RuntimeValue::ArraySlot(index));
                }
                @once Instruction::Index {
                    dst,
                    target,
                    index,
                    default,
                } => {
                    let target = self.array_register(*target);
                    if let Some(value) = self.index_value(target, *index) {
                        self.store_array_element(*dst, value);
                    } else {
                        self.copy_slot(*dst, *default);
                    }
                }
                Instruction::Select {
                    dst,
                    items,
                    index,
                    default,
                } => {
                    let index = self.number_int(*index);
                    let sources = &self.bytecode.value_operands[items.range()];
                    let source = if sources.is_empty() {
                        dst.with_index(*default)
                    } else {
                        sources[clamp_array_index(index, sources.len())]
                    };
                    self.copy_value_slot(*dst, source);
                }
                Instruction::CurveParamSample {
                    dst,
                    source,
                    position,
                    ..
                } => {
                    let position = self.float(*position);
                    self.set_float(
                        *dst,
                        self.params.values.curves[source.0 as usize].sample(position),
                    );
                }
                Instruction::GradientParamSample {
                    dst,
                    source,
                    position,
                    ..
                } => {
                    let position = self.float(*position);
                    let color = sample_gradient(
                        self.params.values.gradients[source.0 as usize].get(),
                        position,
                    );
                    self.set_color(*dst, color);
                }
                @once Instruction::SignalSample {
                    dst,
                    input,
                    seconds,
                    pixel,
                    frame_cache,
                    capability,
                } => {
                    let seconds = self.float(*seconds);
                    let pixel = match *pixel {
                        SignalPixel::Current => SignalPixel::Current,
                        SignalPixel::Local(index) => SignalPixel::Local(self.int(index)),
                        SignalPixel::Global(index) => SignalPixel::Global(self.int(index)),
                    };
                    if self.color_lanes() > 1 {
                        let lanes = self.output.as_ref().map_or(self.color_lanes(), |output| output.len());
                        let mut colors = [Color::BLACK; COLOR_BLOCK_WIDTH];
                        let start = dst.0 as usize * self.color_lanes();
                        let partial = self.lanes.as_ref().is_some_and(|flow| flow.active != Mask::full(lanes));
                        let output = if partial {
                            &mut colors[..lanes]
                        } else {
                            &mut self.workspace.registers.colors[start..start + lanes]
                        };
                        match crate::values::sample_time_from_seconds_f32(seconds) {
                            Ok(time) => self.signal_sampler.sample_block(
                                *capability,
                                *input,
                                time,
                                pixel,
                                output,
                            )?,
                            Err(_) => output.fill(Color::BLACK),
                        }
                        if partial {
                            for lane in self.color_mask() {
                                self.set_color_lane(*dst, lane, colors[lane]);
                            }
                        }
                        continue;
                    }
                    let color = match crate::values::sample_time_from_seconds_f32(seconds) {
                        Ok(sample_time) => self.signal_sampler.sample(
                            *capability,
                            *input,
                            sample_time,
                            pixel,
                            (*frame_cache != u32::MAX).then_some(*frame_cache as usize),
                        )?,
                        Err(_) => black(),
                    };
                    self.set_color(*dst, color);
                }
                Instruction::IntToFloat { dst, src } => {
                    self.set_float(*dst, self.int(*src) as f32);
                }
                Instruction::Not { dst, src } => self.set_bool(*dst, !self.bool(*src)),
                Instruction::NegInt { dst, src } => {
                    let value = self.int(*src).wrapping_neg();
                    self.set_int(*dst, value);
                }
                @once Instruction::NegFloat { dst, src } => {
                    self.float_unary_lanes(*dst, *src, |value| -value);
                }
                @once Instruction::FloatAdd { dst, left, right } => {
                    self.float_binary_lanes(*dst, *left, *right, |a, b| a + b);
                }
                @once Instruction::FloatSubtract { dst, left, right } => {
                    self.float_binary_lanes(*dst, *left, *right, |a, b| a - b);
                }
                @once Instruction::FloatMultiply { dst, left, right } => {
                    self.float_binary_lanes(*dst, *left, *right, |a, b| a * b);
                }
                @once Instruction::FloatMultiplyAdd { dst, left, right, addend } => {
                    lanes::ternary(
                        &mut self.workspace.registers.floats, self.active,
                        self.workspace.numeric_lanes, dst.0, [left.0, right.0, addend.0],
                        |a, b, c| a * b + c,
                    );
                }
                @once Instruction::FloatMultiplyAddConst { dst, value, constant_bits, addend } => {
                    let factor = f32::from_bits(*constant_bits);
                    self.float_binary_lanes(*dst, *value, *addend, |a, b| a * factor + b);
                }
                @once Instruction::FloatMultiplySmoothstep { dst, left, right } => {
                    self.float_binary_lanes(*dst, *left, *right, |a, b| {
                        let t = (a * b).clamp(0.0, 1.0);
                        t * t * (3.0 - 2.0 * t)
                    });
                }
                @once Instruction::FloatDivide { dst, left, right } => {
                    self.float_binary_lanes(*dst, *left, *right, |a, b| a / b);
                }
                @once Instruction::FloatRemainder { dst, left, right } => {
                    self.float_binary_lanes(*dst, *left, *right, |a, b| a % b);
                }
                @once Instruction::FloatAddConst {
                    dst,
                    value,
                    constant_bits,
                } => {
                    let constant = f32::from_bits(*constant_bits);
                    self.float_unary_lanes(*dst, *value, |value| value + constant);
                }
                @once Instruction::FloatSubtractConst {
                    dst,
                    value,
                    constant_bits,
                } => {
                    let constant = f32::from_bits(*constant_bits);
                    self.float_unary_lanes(*dst, *value, |value| value - constant);
                }
                @once Instruction::FloatMultiplyConst {
                    dst,
                    value,
                    constant_bits,
                } => {
                    let constant = f32::from_bits(*constant_bits);
                    self.float_unary_lanes(*dst, *value, |value| value * constant);
                }
                @once Instruction::FloatDivideConst {
                    dst,
                    value,
                    constant_bits,
                } => {
                    let constant = f32::from_bits(*constant_bits);
                    self.float_unary_lanes(*dst, *value, |value| value / constant);
                }
                @once Instruction::FloatRemainderConst {
                    dst,
                    value,
                    constant_bits,
                } => {
                    let constant = f32::from_bits(*constant_bits);
                    self.float_unary_lanes(*dst, *value, |value| value % constant);
                }
                @once Instruction::FloatSubtractFromConst {
                    dst,
                    value,
                    constant_bits,
                } => {
                    let constant = f32::from_bits(*constant_bits);
                    self.float_unary_lanes(*dst, *value, |value| constant - value);
                }
                @once Instruction::FloatDivideIntoConst {
                    dst,
                    value,
                    constant_bits,
                } => {
                    let constant = f32::from_bits(*constant_bits);
                    self.float_unary_lanes(*dst, *value, |value| constant / value);
                }
                @once Instruction::FloatRemainderFromConst {
                    dst,
                    value,
                    constant_bits,
                } => {
                    let constant = f32::from_bits(*constant_bits);
                    self.float_unary_lanes(*dst, *value, |value| constant % value);
                }
                @once Instruction::IntAdd { dst, left, right } => {
                    self.int_binary_lanes(*dst, *left, *right, i32::wrapping_add);
                }
                @once Instruction::IntSubtract { dst, left, right } => {
                    self.int_binary_lanes(*dst, *left, *right, i32::wrapping_sub);
                }
                @once Instruction::IntMultiply { dst, left, right } => {
                    self.int_binary_lanes(*dst, *left, *right, i32::wrapping_mul);
                }
                @once Instruction::IntRemainder { dst, left, right } => {
                    self.int_binary_lanes(*dst, *left, *right, |a, b| a.checked_rem(b).unwrap_or(0));
                }
                Instruction::FloatCompare {
                    dst,
                    op,
                    left,
                    right,
                } => {
                    let left = self.float(*left);
                    let right = self.float(*right);
                    let value = match op {
                        CompareOp::Less => left < right,
                        CompareOp::LessEqual => left <= right,
                        CompareOp::Greater => left > right,
                        CompareOp::GreaterEqual => left >= right,
                    };
                    self.set_bool(*dst, value);
                }
                Instruction::IntCompare {
                    dst,
                    op,
                    left,
                    right,
                } => {
                    let left = self.int(*left);
                    let right = self.int(*right);
                    let value = match op {
                        CompareOp::Less => left < right,
                        CompareOp::LessEqual => left <= right,
                        CompareOp::Greater => left > right,
                        CompareOp::GreaterEqual => left >= right,
                    };
                    self.set_bool(*dst, value);
                }
                Instruction::FloatCompareConst {
                    dst,
                    op,
                    value,
                    constant_bits,
                    constant_left,
                } => {
                    let value = self.float(*value);
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
                    self.set_bool(*dst, value);
                }
                Instruction::ValueEqual {
                    dst,
                    negate,
                    left,
                    right,
                } => {
                    let equal = self.slots_equal(*left, *right);
                    self.set_bool(*dst, if *negate { !equal } else { equal });
                }
                Instruction::EnumParamEqualConst {
                    dst,
                    source,
                    constant,
                    negate,
                    ..
                } => {
                    let equal = self.params.values.enums[source.0 as usize]
                        == self.bytecode.enums[*constant];
                    self.set_bool(*dst, if *negate { !equal } else { equal });
                }
                @branch Instruction::IntJumpLess {
                    left,
                    right,
                    when,
                    target,
                } => {
                    if (self.int(*left) < self.int(*right)) == *when {
                        instructions = &code[*target..];
                    }
                }
                @branch Instruction::IntJumpLessEqual {
                    left,
                    right,
                    when,
                    target,
                } => {
                    if (self.int(*left) <= self.int(*right)) == *when {
                        instructions = &code[*target..];
                    }
                }
                @branch Instruction::IntJumpGreater {
                    left,
                    right,
                    when,
                    target,
                } => {
                    if (self.int(*left) > self.int(*right)) == *when {
                        instructions = &code[*target..];
                    }
                }
                @branch Instruction::IntJumpGreaterEqual {
                    left,
                    right,
                    when,
                    target,
                } => {
                    if (self.int(*left) >= self.int(*right)) == *when {
                        instructions = &code[*target..];
                    }
                }
                @branch Instruction::IntJumpEqual {
                    left,
                    right,
                    when,
                    target,
                } => {
                    if (self.int(*left) == self.int(*right)) == *when {
                        instructions = &code[*target..];
                    }
                }
                @branch Instruction::FloatJumpLess {
                    left,
                    right,
                    when,
                    target,
                } => {
                    if (self.float(*left) < self.float(*right)) == *when {
                        instructions = &code[*target..];
                    }
                }
                @branch Instruction::FloatJumpLessEqual {
                    left,
                    right,
                    when,
                    target,
                } => {
                    if (self.float(*left) <= self.float(*right)) == *when {
                        instructions = &code[*target..];
                    }
                }
                @branch Instruction::FloatJumpGreater {
                    left,
                    right,
                    when,
                    target,
                } => {
                    if (self.float(*left) > self.float(*right)) == *when {
                        instructions = &code[*target..];
                    }
                }
                @branch Instruction::FloatJumpGreaterEqual {
                    left,
                    right,
                    when,
                    target,
                } => {
                    if (self.float(*left) >= self.float(*right)) == *when {
                        instructions = &code[*target..];
                    }
                }
                @branch Instruction::FloatJumpEqual {
                    left,
                    right,
                    when,
                    target,
                } => {
                    if (self.float(*left) == self.float(*right)) == *when {
                        instructions = &code[*target..];
                    }
                }
                @branch Instruction::FloatJumpLessConst {
                    value,
                    constant_bits,
                    when,
                    target,
                } => {
                    if (self.float(*value) < f32::from_bits(*constant_bits)) == *when {
                        instructions = &code[*target..];
                    }
                }
                @branch Instruction::FloatJumpLessEqualConst {
                    value,
                    constant_bits,
                    when,
                    target,
                } => {
                    if (self.float(*value) <= f32::from_bits(*constant_bits)) == *when {
                        instructions = &code[*target..];
                    }
                }
                @branch Instruction::FloatJumpGreaterConst {
                    value,
                    constant_bits,
                    when,
                    target,
                } => {
                    if (self.float(*value) > f32::from_bits(*constant_bits)) == *when {
                        instructions = &code[*target..];
                    }
                }
                @branch Instruction::FloatJumpGreaterEqualConst {
                    value,
                    constant_bits,
                    when,
                    target,
                } => {
                    if (self.float(*value) >= f32::from_bits(*constant_bits)) == *when {
                        instructions = &code[*target..];
                    }
                }
                @branch Instruction::FloatJumpEqualConst {
                    value,
                    constant_bits,
                    when,
                    target,
                } => {
                    if (self.float(*value) == f32::from_bits(*constant_bits)) == *when {
                        instructions = &code[*target..];
                    }
                }
                @once Instruction::Jump(target) => {
                    if let Some(flow) = &mut self.lanes {
                        for lane in flow.active {
                            flow.redirect(lane, *target);
                        }
                        let Some(next) = flow.resume() else {
                            unreachable!("jump retains active lanes")
                        };
                        self.active = flow.active;
                        next_event = flow.next_join_remaining(code.len());
                        instructions = &code[next..];
                    } else {
                        instructions = &code[*target..];
                    }
                }
                @branch Instruction::JumpIfFalse { condition, target } => {
                    if !self.bool(*condition) {
                        instructions = &code[*target..];
                    }
                }
                @branch Instruction::JumpIfTrue { condition, target } => {
                    if self.bool(*condition) {
                        instructions = &code[*target..];
                    }
                }
                @branch Instruction::LoopRangeStart {
                    id,
                    count,
                    cap,
                    end,
                } => {
                    let count = self.int(*count).max(0).min(*cap);
                    // Loop IDs are checked at admission; new() reserves loop_count.
                    let index = self.numeric_index(*id);
                    let remaining = &mut self.workspace.loop_remaining[index];
                    *remaining = count as usize;
                    if count == 0 {
                        instructions = &code[end + 1..];
                    }
                }
                @once Instruction::LoopMarksStart { id, marks, end } => {
                    let count = self.mark_value(*marks).marks.len();
                    // Loop IDs are checked at admission; new() reserves loop_count.
                    let index = self.numeric_index(*id);
                    let remaining = &mut self.workspace.loop_remaining[index];
                    *remaining = count;
                    if count == 0 {
                        instructions = &code[end + 1..];
                    }
                }
                @branch Instruction::LoopEnd { id, start } => {
                    // Loop IDs are checked at admission; new() reserves loop_count.
                    let index = self.numeric_index(*id);
                    let remaining = &mut self.workspace.loop_remaining[index];
                    if *remaining > 1 {
                        *remaining -= 1;
                        instructions = &code[*start..];
                    } else {
                        *remaining = 0;
                    }
                }
                Instruction::ContextRead { dst, read } => {
                    self.context_read(*dst, *read);
                }
                Instruction::QuerySeconds { dst, seconds } => {
                    let value = crate::values::sample_time_from_seconds_f32(self.float(*seconds))
                        .ok()
                        .filter(|time| time.as_ticks() < self.run_context().duration.as_ticks())
                        .map_or(f32::NAN, crate::values::sample_time_seconds_f32);
                    self.set_float(*dst, value);
                }
                Instruction::QueryProgress { dst, seconds } => {
                    let duration = self.run_context().duration.as_ticks();
                    let value = crate::values::sample_time_from_seconds_f32(self.float(*seconds))
                        .ok()
                        .filter(|time| time.as_ticks() < duration)
                        .map_or(f32::NAN, |time| (time.as_ticks() as f32 / duration as f32).clamp(0.0, 1.0));
                    self.set_float(*dst, value);
                }
                Instruction::SectionPosition {
                    dst,
                    width,
                    inverse,
                } => {
                    let width = self.float(*width);
                    let inverse = self.float(*inverse);
                    let index = self.run_context().pixel_index as f32;
                    let value = (index - libm::floorf(index * inverse) * width) * inverse;
                    self.set_float(*dst, value);
                }
                Instruction::SectionQuery { dst, width, index } => {
                    self.set_int(*dst, self.section_context().query(self.int(*width), *index));
                }
                @once Instruction::FloatUnary { dst, op: FloatUnary::Abs, value } => {
                    self.float_unary_lanes(*dst, *value, |value| if value.is_nan() { f32::NAN } else { value.abs() });
                }
                @once Instruction::FloatUnary { dst, op: FloatUnary::Floor, value } => {
                    self.float_unary_lanes(*dst, *value, |value| if value.is_nan() { f32::NAN } else { libm::floorf(value) });
                }
                @once Instruction::FloatUnary { dst, op, value } => {
                    self.float_unary_lanes(*dst, *value, |value| if value.is_nan() {
                        f32::NAN
                    } else {
                        match op {
                            FloatUnary::Sin => micromath::F32Ext::sin(value),
                            FloatUnary::Cos => micromath::F32Ext::cos(value),
                            FloatUnary::Abs => value.abs(),
                            FloatUnary::Floor => libm::floorf(value),
                            FloatUnary::Sqrt => libm::sqrtf(value),
                        }
                    });
                }
                @once Instruction::FloatBinary { dst, op: FloatBinary::Min, left, right } => {
                    self.float_binary_lanes(*dst, *left, *right, |a, b| float_binary(FloatBinary::Min, a, b));
                }
                @once Instruction::FloatBinary { dst, op: FloatBinary::Max, left, right } => {
                    self.float_binary_lanes(*dst, *left, *right, |a, b| float_binary(FloatBinary::Max, a, b));
                }
                @once Instruction::FloatBinary {
                    dst,
                    op,
                    left,
                    right,
                } => {
                    self.float_binary_lanes(*dst, *left, *right, |left, right| float_binary(*op, left, right));
                }
                @once Instruction::FloatBinaryConst { dst, op: FloatBinary::Min, value, constant_bits } => {
                    let constant = f32::from_bits(*constant_bits);
                    self.float_unary_lanes(*dst, *value, |value| float_binary(FloatBinary::Min, value, constant));
                }
                @once Instruction::FloatBinaryConst { dst, op: FloatBinary::Max, value, constant_bits } => {
                    let constant = f32::from_bits(*constant_bits);
                    self.float_unary_lanes(*dst, *value, |value| float_binary(FloatBinary::Max, value, constant));
                }
                @once Instruction::FloatBinaryConst {
                    dst,
                    op,
                    value,
                    constant_bits,
                } => {
                    let constant = f32::from_bits(*constant_bits);
                    self.float_unary_lanes(*dst, *value, |value| float_binary(*op, value, constant));
                }
                Instruction::Clamp {
                    dst,
                    value,
                    min,
                    max,
                } => {
                    let value = self.float(*value);
                    let min = self.float(*min);
                    let max = self.float(*max);
                    self.set_float(*dst, clamp_float(value, min, max));
                }
                @once Instruction::ClampConst {
                    dst,
                    value,
                    min_bits,
                    max_bits,
                } => {
                    let min = f32::from_bits(*min_bits);
                    let max = f32::from_bits(*max_bits);
                    self.float_unary_lanes(*dst, *value, |value| clamp_float(value, min, max));
                }
                @once Instruction::Smoothstep { dst, value } => {
                    self.float_unary_lanes(*dst, *value, |value| {
                        let t = value.clamp(0.0, 1.0);
                        t * t * (3.0 - 2.0 * t)
                    });
                }
                Instruction::MixFloat {
                    dst,
                    left,
                    right,
                    amount,
                } => {
                    let amount = self.float(*amount);
                    let left = self.float(*left);
                    let right = self.float(*right);
                    self.set_float(*dst, left + (right - left) * amount);
                }
                @once Instruction::MixColor {
                    dst,
                    left,
                    right,
                    amount,
                } => {
                    for lane in self.color_mask() {
                        self.lane = if self.lanes.is_some() { lane } else { 0 };
                        let amount = self.float(*amount);
                        let left = self.color_lane(*left, lane);
                        let right = self.color_lane(*right, lane);
                        self.set_color_lane(*dst, lane, mix_colors(left, right, amount));
                    }
                }
                @once Instruction::ColorBinary {
                    dst,
                    op,
                    left,
                    right,
                } => {
                    for lane in self.color_mask() {
                        let left = self.color_lane(*left, lane);
                        let right = self.color_lane(*right, lane);
                        let color = match op {
                            ColorBinary::Add => add_colors(left, right),
                            ColorBinary::Multiply => multiply_colors(left, right),
                            ColorBinary::Max => max_colors(left, right),
                        };
                        self.set_color_lane(*dst, lane, color);
                    }
                }
                @once Instruction::ColorScale { dst, color, scale } => {
                    for lane in self.color_mask() {
                        self.lane = if self.lanes.is_some() { lane } else { 0 };
                        let scale = self.float(*scale);
                        let color = self.color_lane(*color, lane);
                        self.set_color_lane(*dst, lane, scale_color(color, scale));
                    }
                }
                Instruction::ColorComponent { dst, op, color } => {
                    let color = self.color(*color);
                    let value = match op {
                        ColorComponent::Hue => color_hue(color),
                        ColorComponent::Saturation => color_saturation(color),
                        ColorComponent::Intensity => color_intensity(color),
                    };
                    self.set_float(*dst, value);
                }
                @once Instruction::ColorInvert { dst, color } => {
                    for lane in self.color_mask() {
                        let color = self.color_lane(*color, lane);
                        self.set_color_lane(*dst, lane, invert_color(color));
                    }
                }
                Instruction::Rgb {
                    dst,
                    red,
                    green,
                    blue,
                } => {
                    let (red, green, blue) =
                        (self.float(*red), self.float(*green), self.float(*blue));
                    self.set_color(*dst, crate::sampling::rgb(red, green, blue));
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
                            self.float(*hue),
                            self.float(*saturation),
                            self.float(*value),
                        ),
                    );
                }
                Instruction::Rand { dst, seed } => {
                    let random = crate::sampling::deterministic_random_seed(self.float(*seed));
                    self.set_float(*dst, random);
                }
                @once Instruction::CurveFloatClamped {
                    dst,
                    curve,
                    position,
                    min,
                    max,
                } => {
                    let curve = self.curve_value(*curve).raw();
                    let position = self.float(*position);
                    let min = self.float(*min);
                    let max = self.float(*max);
                    self.set_float(*dst, clamp_float(sample_curve(curve, position), min, max));
                }
                Instruction::CurveParamFloatClamped {
                    dst,
                    source,
                    position,
                    min,
                    max,
                    ..
                } => {
                    let position = self.float(*position);
                    let min = self.float(*min);
                    let max = self.float(*max);
                    let value = clamp_float(
                        self.params.values.curves[source.0 as usize].sample(position),
                        min,
                        max,
                    );
                    self.set_float(*dst, value);
                }
                @once Instruction::GradientColorScaled {
                    dst,
                    gradient,
                    position,
                    scale,
                } => {
                    let scale = self.float(*scale).clamp(0.0, 1.0);
                    if scale <= 0.0 {
                        self.set_color(*dst, black());
                    } else {
                        let gradient = self.gradient_value(*gradient);
                        let position = self.float(*position);
                        let color = sample_gradient(gradient, position);
                        self.set_color(*dst, scale_color(color, scale));
                    }
                }
                Instruction::GradientParamColorScaled {
                    dst,
                    source,
                    position,
                    scale,
                    ..
                } => {
                    let scale = self.float(*scale).clamp(0.0, 1.0);
                    if scale <= 0.0 {
                        self.set_color(*dst, black());
                    } else {
                        let position = self.float(*position);
                        let gradient = self.params.values.gradients[source.0 as usize].get();
                        let color = sample_gradient(gradient, position);
                        self.set_color(*dst, scale_color(color, scale));
                    }
                }
                @once Instruction::CurveCrossing {
                    dst,
                    curve,
                    value,
                    before,
                } => {
                    let curve = self.curve_value(*curve).raw();
                    let value = self.float(*value);
                    let result = match before {
                        Some(position) => crate::sampling::curve_last_crossing(
                            curve,
                            value,
                            self.float(*position),
                        ),
                        None => curve_crossing_raw(curve, value, f32::NAN),
                    };
                    self.set_float(*dst, result);
                }
                Instruction::CurveParamCrossing {
                    dst,
                    source,
                    value,
                    before,
                    ..
                } => {
                    let value = self.float(*value);
                    let curve = &self.params.values.curves[source.0 as usize];
                    let result = match before {
                        Some(position) => crate::sampling::curve_last_crossing(
                            curve.raw(),
                            value,
                            self.float(*position),
                        ),
                        None => curve.crossing(value, f32::NAN),
                    };
                    self.set_float(*dst, result);
                }
                @once Instruction::Len { dst, value } => {
                    let length = self
                        .array_register(*value)
                        .view(&self.workspace.arrays)
                        .len();
                    self.set_int(*dst, int_len(length));
                }
                @once Instruction::Mark { marks, op } => {
                    let marks = self.mark_value(*marks);
                    match *op {
                        MarkOp::Count { dst } => {
                            self.set_int(dst, int_len(marks.marks.len()));
                        }
                        MarkOp::At { dst, index } => {
                            self.set_float(dst, mark_at_from(marks, self.int(index)));
                        }
                        MarkOp::Last { dst, seconds } => {
                            let mark = previous_mark(marks, self.float(seconds));
                            self.set_float(
                                dst,
                                mark.map_or(f32::NAN, |(_, time)| {
                                    sample_duration_seconds_f32(time)
                                }),
                            );
                        }
                        MarkOp::LastIndex { dst, seconds } => {
                            self.set_int(dst, prev_index(marks, self.float(seconds)));
                        }
                    }
                }
                @once Instruction::ReturnColor(value) => {
                    if let Some(flow) = &self.lanes {
                        for lane in flow.active {
                            let color = self.color_lane((*value).into(), lane);
                            if let Some(output) = &mut self.output {
                                output[lane] = color;
                            }
                        }
                        if let Some(flow) = &mut self.lanes
                            && let Some(next) = flow.resume()
                        {
                            self.active = flow.active;
                            next_event = flow.next_join_remaining(code.len());
                            instructions = &code[next..];
                            continue;
                        }
                    }
                    return Ok(*value);
                }
            }
        }
    }

    #[inline(always)]
    fn float_binary_lanes(
        &mut self,
        dst: FloatSlot,
        left: FloatSlot,
        right: FloatSlot,
        op: impl Fn(f32, f32) -> f32,
    ) {
        let mask = self.numeric_mask();
        lanes::binary(
            &mut self.workspace.registers.floats,
            mask,
            self.workspace.numeric_lanes,
            dst.0,
            left.0,
            right.0,
            op,
        );
    }

    #[inline(always)]
    fn float_unary_lanes(&mut self, dst: FloatSlot, src: FloatSlot, op: impl Fn(f32) -> f32) {
        let mask = self.numeric_mask();
        lanes::unary(
            &mut self.workspace.registers.floats,
            mask,
            self.workspace.numeric_lanes,
            dst.0,
            src.0,
            op,
        );
    }

    #[inline(always)]
    fn int_binary_lanes(
        &mut self,
        dst: IntSlot,
        left: IntSlot,
        right: IntSlot,
        op: impl Fn(i32, i32) -> i32,
    ) {
        let mask = self.numeric_mask();
        lanes::binary(
            &mut self.workspace.registers.ints,
            mask,
            self.workspace.numeric_lanes,
            dst.0,
            left.0,
            right.0,
            op,
        );
    }

    fn int(&self, slot: IntSlot) -> i32 {
        self.workspace.registers.ints[self.numeric_index(slot.0)]
    }

    fn float(&self, slot: FloatSlot) -> f32 {
        self.workspace.registers.floats[self.numeric_index(slot.0)]
    }

    fn bool(&self, slot: BoolSlot) -> bool {
        self.workspace.registers.bools[self.numeric_index(slot.0)]
    }

    fn numeric_index(&self, slot: u32) -> usize {
        slot as usize * self.workspace.numeric_lanes + self.lane
    }

    fn numeric_mask(&self) -> Mask {
        self.active
    }

    fn color_mask(&self) -> Mask {
        if self.broadcast {
            Mask::FIRST
        } else {
            self.lanes
                .as_ref()
                .map_or_else(|| Mask::full(self.color_lanes()), |flow| flow.active)
        }
    }

    fn run_context(&self) -> &RunContext {
        self.lane_contexts
            .map_or(self.context, |contexts| &contexts[self.lane].context)
    }

    fn spatial_context(&self) -> &SpatialContext {
        self.lane_contexts
            .map_or(self.spatial, |contexts| &contexts[self.lane].spatial)
    }

    fn section_context(&self) -> crate::sections::SectionContext<'_> {
        self.lane_contexts
            .map_or(self.sections, |contexts| contexts[self.lane].sections)
    }

    fn color_lanes(&self) -> usize {
        self.workspace.color_lanes
    }

    #[inline(always)]
    fn color(&self, slot: ColorSlot) -> Color {
        self.color_lane(slot, self.lane)
    }

    #[inline(always)]
    fn color_lane(&self, slot: ColorSlot, lane: usize) -> Color {
        self.workspace.registers.colors[slot.0 as usize * self.color_lanes() + lane]
    }

    #[inline(always)]
    fn set_color_lane(&mut self, slot: ColorSlot, lane: usize, value: Color) {
        let index = slot.0 as usize * self.color_lanes() + lane;
        self.workspace.registers.colors[index] = value;
    }

    fn array_register(&self, slot: ArraySlot) -> &ArrayRegister {
        &self.workspace.registers.array_values[slot.0 as usize]
    }

    fn value(&self, slot: ValueSlot) -> RuntimeValue {
        match slot {
            ValueSlot::Int(slot) => RuntimeValue::Int(self.int(slot)),
            ValueSlot::Float(slot) => RuntimeValue::Float(self.float(slot)),
            ValueSlot::Bool(slot) => RuntimeValue::Bool(self.bool(slot)),
            ValueSlot::Color(slot) => RuntimeValue::Color(self.color(slot)),
            ValueSlot::Array(slot) => self.array_register(slot).runtime(),
            ValueSlot::Void => RuntimeValue::Void,
            ValueSlot::Enum(slot) => {
                RuntimeValue::Enum(self.workspace.registers.enums[slot.0 as usize].clone())
            }
            ValueSlot::Curve(slot) => self.curve_value(slot).runtime(),
            ValueSlot::Gradient(slot) => {
                RuntimeValue::Gradient(self.workspace.registers.gradients[slot.0 as usize].owned())
            }
            ValueSlot::Marks(slot) => {
                RuntimeValue::Marks(self.workspace.registers.marks[slot.0 as usize].owned())
            }
        }
    }

    fn curve_value(&self, slot: CurveSlot) -> &CurveRegister {
        &self.workspace.registers.curves[slot.0 as usize]
    }

    fn gradient_value(&self, slot: GradientSlot) -> &Gradient {
        self.workspace.registers.gradients[slot.0 as usize].get()
    }

    fn set_curve(&mut self, slot: CurveSlot, value: CurveRegister) {
        self.workspace.registers.curves[slot.0 as usize] = value;
    }

    fn set_gradient(&mut self, slot: GradientSlot, value: GradientRegister) {
        self.workspace.registers.gradients[slot.0 as usize] = value;
    }

    fn mark_value(&self, slot: MarksSlot) -> &Marks {
        self.workspace.registers.marks[slot.0 as usize].get()
    }

    fn set_marks(&mut self, slot: MarksSlot, value: MarksRegister) {
        self.workspace.registers.marks[slot.0 as usize] = value;
    }

    fn number_int(&self, slot: NumberSlot) -> i32 {
        match slot {
            NumberSlot::Int(slot) => self.int(slot),
            NumberSlot::Float(slot) => self.float(slot) as i32,
        }
    }

    fn set_int(&mut self, slot: IntSlot, value: i32) {
        let index = self.numeric_index(slot.0);
        self.workspace.registers.ints[index] = value;
    }

    fn set_float(&mut self, slot: FloatSlot, value: f32) {
        let index = self.numeric_index(slot.0);
        self.workspace.registers.floats[index] = value;
    }

    fn set_bool(&mut self, slot: BoolSlot, value: bool) {
        let index = self.numeric_index(slot.0);
        self.workspace.registers.bools[index] = value;
    }

    #[inline(never)]
    fn set_color(&mut self, slot: ColorSlot, value: Color) {
        if self.lanes.is_some() {
            self.set_color_lane(slot, self.lane, value);
            return;
        }
        let lanes = self.color_lanes();
        let start = slot.0 as usize * lanes;
        self.workspace.registers.colors[start..start + lanes].fill(value);
    }

    fn set_enum(&mut self, slot: EnumSlot, value: Identifier) {
        self.workspace.registers.enums[slot.0 as usize] = value;
    }

    fn set_array(&mut self, slot: ArraySlot, value: ArrayRegister) {
        if let ArrayRegister::Local(index) = &value {
            self.workspace
                .arrays
                .retain(&RuntimeValue::ArraySlot(*index));
        }
        let old = core::mem::replace(
            &mut self.workspace.registers.array_values[slot.0 as usize],
            value,
        );
        if let ArrayRegister::Local(index) = old {
            self.workspace
                .arrays
                .release(RuntimeValue::ArraySlot(index));
        }
    }

    /// Binding checks the elements of authored arrays; bytecode admission checks
    /// each calculated array's inputs and that Index's destination accepts its
    /// element type. Array snapshots and forwarding preserve that relationship.
    /// The value therefore identifies its bank. Only int-to-float widening needs
    /// the destination kind; no wrong-type execution branch is possible here.
    fn store_array_element(&mut self, destination: ValueSlot, value: RuntimeValue) {
        let index = destination.index();
        match value {
            RuntimeValue::Void => {}
            RuntimeValue::Int(value) => {
                if matches!(destination, ValueSlot::Float(_)) {
                    self.set_float(FloatSlot(index), value as f32);
                } else {
                    self.set_int(IntSlot(index), value);
                }
            }
            RuntimeValue::Float(value) => self.set_float(FloatSlot(index), value),
            RuntimeValue::Bool(value) => self.set_bool(BoolSlot(index), value),
            RuntimeValue::Color(value) => self.set_color(ColorSlot(index), value),
            RuntimeValue::Enum(value) => self.set_enum(EnumSlot(index), value),
            RuntimeValue::Curve(value) => {
                self.set_curve(CurveSlot(index), CurveRegister::Raw(value))
            }
            RuntimeValue::PreparedCurve(value) => {
                self.set_curve(CurveSlot(index), CurveRegister::Prepared(value))
            }
            RuntimeValue::Gradient(value) => {
                self.set_gradient(GradientSlot(index), GradientRegister::Shared(value))
            }
            RuntimeValue::Marks(value) => {
                self.set_marks(MarksSlot(index), MarksRegister::Shared(value))
            }
            RuntimeValue::Array(values) => {
                self.set_array(ArraySlot(index), ArrayRegister::Shared(values))
            }
            RuntimeValue::ArraySlot(slot) => {
                self.set_array(ArraySlot(index), ArrayRegister::Local(slot))
            }
        }
    }

    /// Admission proves each operand has the destination's type or widens int to float.
    fn copy_value_slot(&mut self, dst: ValueSlot, src: ValueSlot) {
        match (dst, src) {
            (ValueSlot::Float(dst), ValueSlot::Int(src)) => {
                self.set_float(dst, self.int(src) as f32)
            }
            (dst, src) => self.copy_slot(dst, src.index()),
        }
    }

    fn copy_slot(&mut self, dst: ValueSlot, src: u32) {
        match dst {
            ValueSlot::Enum(dst) => {
                self.set_enum(dst, self.workspace.registers.enums[src as usize].clone())
            }
            ValueSlot::Int(dst) => self.set_int(dst, self.int(IntSlot(src))),
            ValueSlot::Float(dst) => self.set_float(dst, self.float(FloatSlot(src))),
            ValueSlot::Bool(dst) => self.set_bool(dst, self.bool(BoolSlot(src))),
            ValueSlot::Color(dst) => {
                if self.lanes.is_some() {
                    self.set_color(dst, self.color(ColorSlot(src)));
                    return;
                }
                let lanes = self.color_lanes();
                let start = src as usize * lanes;
                self.workspace
                    .registers
                    .colors
                    .copy_within(start..start + lanes, dst.0 as usize * lanes);
            }
            ValueSlot::Array(dst) => {
                self.set_array(dst, self.array_register(ArraySlot(src)).clone())
            }
            ValueSlot::Void => {}
            ValueSlot::Curve(dst) => {
                self.set_curve(dst, self.workspace.registers.curves[src as usize].clone())
            }
            ValueSlot::Gradient(dst) => self.set_gradient(
                dst,
                self.workspace.registers.gradients[src as usize].clone(),
            ),
            ValueSlot::Marks(dst) => {
                self.set_marks(dst, self.workspace.registers.marks[src as usize].clone())
            }
        }
    }

    fn context_read(&mut self, dst: NumberSlot, read: C) {
        match read.read(self.run_context(), self.spatial_context()) {
            context::Number::Int(value) => self.set_context_int(dst, value),
            context::Number::Float(value) => self.set_context_float(dst, value),
        }
    }

    fn set_context_float(&mut self, dst: NumberSlot, value: f32) {
        match dst {
            NumberSlot::Float(slot) => self.set_float(slot, value),
            NumberSlot::Int(slot) => self.set_int(slot, value as i32),
        }
    }

    fn set_context_int(&mut self, dst: NumberSlot, value: i32) {
        match dst {
            NumberSlot::Int(slot) => self.set_int(slot, value),
            NumberSlot::Float(slot) => self.set_float(slot, value as f32),
        }
    }

    fn index_value(&self, target: &ArrayRegister, index: NumberSlot) -> Option<RuntimeValue> {
        let view = target.view(&self.workspace.arrays);
        if view.len() == 0 {
            None
        } else {
            view.get(clamp_array_index(self.number_int(index), view.len()))
        }
    }

    fn slots_equal(&self, left: ValueSlot, right: ValueSlot) -> bool {
        match (left, right) {
            (ValueSlot::Int(left), ValueSlot::Int(right)) => self.int(left) == self.int(right),
            (ValueSlot::Float(left), ValueSlot::Float(right)) => {
                self.float(left) == self.float(right)
            }
            (ValueSlot::Int(left), ValueSlot::Float(right)) => {
                self.int(left) as f32 == self.float(right)
            }
            (ValueSlot::Float(left), ValueSlot::Int(right)) => {
                self.float(left) == self.int(right) as f32
            }
            (ValueSlot::Bool(left), ValueSlot::Bool(right)) => self.bool(left) == self.bool(right),
            (ValueSlot::Color(left), ValueSlot::Color(right)) => {
                self.color(left) == self.color(right)
            }
            (ValueSlot::Enum(left), ValueSlot::Enum(right)) => {
                self.workspace.registers.enums[left.0 as usize]
                    == self.workspace.registers.enums[right.0 as usize]
            }
            // Arrays have no identity equality in the DSL.
            (ValueSlot::Array(_), ValueSlot::Array(_)) => false,
            _ => {
                let left = self.value(left);
                let right = self.value(right);
                runtime_refs_equal(&left, &right)
            }
        }
    }
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
        .and_then(|index| marks.marks.get(index))
        .map(|mark| sample_duration_seconds_f32(*mark))
        .unwrap_or(f32::NAN)
}

fn previous_mark(marks: &Marks, seconds: f32) -> Option<(usize, SampleDuration)> {
    marks
        .marks
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, mark)| sample_duration_seconds_f32(*mark) <= seconds)
        .max_by_key(|(index, mark)| (mark.as_ticks(), *index))
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
    fn materializing_accepted_values_matches_checked_binding() {
        let types = [Type::Int, Type::Float, Type::Bool, Type::array(Type::Int)];
        let values = [
            Value::Int(-7),
            Value::Int(3),
            Value::Bool(true),
            Value::Array(Arc::from(vec![Value::Int(4), Value::Int(9)])),
        ];
        let mut cache = DslBindCache::default();
        let checked = BoundParams::bind_values(&types, values.to_vec(), &mut cache).unwrap();
        let prepared = BoundParams::from_values(types.iter().zip(values), &mut cache);
        assert_eq!(
            prepared.iter_values().collect::<alloc::vec::Vec<_>>(),
            checked.iter_values().collect::<alloc::vec::Vec<_>>()
        );
        assert!(matches!(prepared.value(1), Ok(Value::Float(3.0))));
    }

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

#[cfg(test)]
mod mark_totality_tests {
    use super::{int_len, mark_at_from, prev_index, previous_mark};
    use crate::values::{Marks, SampleDuration};
    use alloc::vec;

    #[test]
    fn mark_at_returns_nan_for_negative_and_out_of_range_indices() {
        let marks = Marks {
            marks: vec![SampleDuration::from_ticks(1_000_000)],
        };
        assert!(mark_at_from(&marks, -1).is_nan());
        assert!(mark_at_from(&marks, 1).is_nan());
        assert_eq!(mark_at_from(&marks, 0), 1.0);
    }

    #[test]
    fn mark_queries_have_defined_indices_and_times() {
        let marks = Marks {
            marks: vec![
                SampleDuration::from_ticks(500_000),
                SampleDuration::from_ticks(1_000_000),
                SampleDuration::from_ticks(1_500_000),
            ],
        };
        assert_eq!(prev_index(&marks, 1.2), 1);
        assert_eq!(
            previous_mark(&marks, 1.0),
            Some((1, SampleDuration::from_ticks(1_000_000)))
        );
        assert_eq!(previous_mark(&marks, 0.0), None);
        assert_eq!(prev_index(&marks, f32::NAN), -1);
        assert_eq!(prev_index(&marks, f32::NEG_INFINITY), -1);
        assert_eq!(prev_index(&marks, f32::INFINITY), 2);
        assert_eq!(int_len(usize::MAX), i32::MAX);
    }
}
