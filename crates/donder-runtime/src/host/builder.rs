//! Invariant-preserving show construction. Checked authored inputs enter before
//! `build`; graph references and storage addresses are issued only by its
//! owner, which writes the image's sections.
use alloc::{collections::BTreeSet, vec, vec::Vec};
use core::marker::PhantomData;
use donder_runtime_types::bytecode::{
    Banks, BytecodeProgram, Instruction, ParamStorage, ParameterKind, param_bank,
};
use donder_runtime_types::image::{self, NONE, Range, SectionsWriter};
use donder_runtime_types::{
    AutomatedQuantity, AutomationMapping, Curve, FixtureGeometry, Gradient, Identifier,
    InstructionWord, Marks, OperatorInvocation, OperatorProgram, OutputEncoding, PixelEncoding,
    PreparedAutomation, SampleDuration, SampleInvocation, SampleProgram, SampleTime,
    SequenceTiming, Shared, SpatialContext, TargetGeometry, TargetScope, Type, Value,
};

use crate::targets::PreparedPixel;

macro_rules! handle {
    ($($name:ident),+ $(,)?) => {$(
        #[derive(Clone, Copy)]
        pub struct $name<'id> {
            index: usize,
            brand: PhantomData<fn(&'id ()) -> &'id ()>,
        }
        impl<'id> $name<'id> {
            fn new(index: usize) -> Self { Self { index, brand: PhantomData } }
        }
    )+};
}
handle!(
    FixtureHandle,
    TargetHandle,
    EffectHandle,
    SignalHandle,
    OutputHandle,
    LookupHandle
);

/// The signal a built show displays, or black.
pub struct SequenceRoot<'id>(Option<SignalHandle<'id>>);

#[derive(Clone, Copy)]
pub struct WindowHandle<'id> {
    start: SampleTime,
    duration: SampleDuration,
    brand: PhantomData<fn(&'id ()) -> &'id ()>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VoiceLength {
    /// Plays for this long from each start.
    Fixed(SampleDuration),
    /// Plays until stopped.
    Open,
}

struct Target {
    pixels: Vec<PreparedPixel>,
    spatial: Vec<SpatialContext>,
    source_pixels: Vec<(usize, u32)>,
    selection: Shared<[(usize, u32)]>,
    scope: TargetScope,
}

struct Output {
    controller_index: u32,
    port: u32,
    bytes: usize,
}

struct Layer {
    enabled: bool,
    effects: Vec<u32>,
}

struct Node {
    kind: u32,
    layer: u32,
    program: u32,
    params: u32,
    inputs: Vec<u32>,
    automation: u32,
}

/// Resources written so far, so that equal values share one record.
#[derive(Default)]
struct Interner {
    curves: Vec<(Shared<Curve>, u32)>,
    gradients: Vec<(Shared<Gradient>, u32)>,
    tracks: Vec<(Shared<[u32]>, Range)>,
    marks: Vec<image::Marks>,
    arrays: Vec<(Range, u32)>,
}

pub struct SequenceBuilder<'id> {
    timing: SequenceTiming,
    w: SectionsWriter,
    interner: Interner,
    fixtures: Vec<image::Fixture>,
    geometry: Vec<FixtureGeometry>,
    storage: Vec<usize>,
    pixel_count: usize,
    targets: Vec<Target>,
    sample_programs: Vec<(Shared<SampleProgram>, u32)>,
    operator_programs: Vec<(Shared<OperatorProgram>, u32)>,
    clips: Vec<image::Clip>,
    layers: Vec<Layer>,
    nodes: Vec<Node>,
    outputs: Vec<Output>,
    collections: Vec<Vec<image::Reader>>,
    controls: Vec<Vec<image::ControlTarget>>,
    brand: PhantomData<fn(&'id ()) -> &'id ()>,
}

/// Append `items` to `section`; their range there.
fn push<T: Copy>(section: &mut Vec<T>, items: impl IntoIterator<Item = T>) -> Range {
    let start = section.len();
    section.extend(items);
    Range {
        start: start as u32,
        len: (section.len() - start) as u32,
    }
}

/// Store `items` once: reuse an equal run already in `section`.
fn shared<T: Copy + PartialEq>(section: &mut Vec<T>, known: &mut Vec<Range>, items: &[T]) -> Range {
    if let Some(range) = known
        .iter()
        .find(|range| range.get(section) == items)
        .copied()
    {
        return range;
    }
    let range = push(section, items.iter().copied());
    known.push(range);
    range
}

fn counts(banks: Banks) -> image::BankCounts {
    image::BankCounts {
        counts: [
            banks.floats,
            banks.ints,
            banks.bools,
            banks.colors,
            banks.resources,
            0,
        ],
    }
}

/// A bound color word: red, green and blue in its low bytes.
fn color_word(color: donder_runtime_types::Color) -> u32 {
    u32::from_le_bytes([color.red, color.green, color.blue, 0])
}

fn enum_index(enums: &[Identifier], name: &Identifier) -> u32 {
    enums
        .iter()
        .position(|option| option == name)
        .map_or(-1, |index| index as i32) as u32
}

impl SequenceBuilder<'_> {
    fn curve(&mut self, curve: &Shared<Curve>) -> u32 {
        if let Some((_, index)) = self
            .interner
            .curves
            .iter()
            .find(|(existing, _)| Shared::ptr_eq(existing, curve) || existing.same(curve))
        {
            return *index;
        }
        let w = &mut self.w;
        let points = push(&mut w.curve_points, curve.points.iter().copied());
        let mut areas = vec![0.0; curve.points.len()];
        donder_runtime_types::sampling::curve_prefix_areas(&curve.points, &mut areas);
        let areas = push(&mut w.curve_areas, areas);
        let mut crossings = vec![image::CrossingSegment::default(); curve.points.len().max(1)];
        let (kind, len) = crate::resources::curve_crossings(&curve.points, &mut crossings);
        let crossings = push(&mut w.crossings, crossings[..len].iter().copied());
        let index = w.curves.len() as u32;
        w.curves.push(image::Curve {
            points,
            areas,
            crossings_kind: kind,
            crossings,
        });
        self.interner.curves.push((Shared::clone(curve), index));
        index
    }

    fn gradient(&mut self, gradient: &Shared<Gradient>) -> u32 {
        if let Some((_, index)) = self
            .interner
            .gradients
            .iter()
            .find(|(existing, _)| Shared::ptr_eq(existing, gradient) || existing.same(gradient))
        {
            return *index;
        }
        let w = &mut self.w;
        let stops = push(
            &mut w.gradient_stops,
            gradient.stops.iter().map(|stop| image::GradientStop {
                position: stop.position,
                color: [stop.color.red, stop.color.green, stop.color.blue, 0],
            }),
        );
        let index = w.gradients.len() as u32;
        w.gradients.push(image::Gradient { stops });
        self.interner
            .gradients
            .push((Shared::clone(gradient), index));
        index
    }

    fn marks(&mut self, marks: &Marks) -> u32 {
        let track = marks.track();
        let range = match self
            .interner
            .tracks
            .iter()
            .find(|(existing, _)| Shared::ptr_eq(existing, track) || **existing == **track)
        {
            Some((_, range)) => *range,
            None => {
                let range = push(&mut self.w.tracks, track.iter().copied());
                self.interner.tracks.push((Shared::clone(track), range));
                range
            }
        };
        let (first, end, origin) = marks.bounds();
        let record = image::Marks {
            track: range,
            first,
            end,
            origin,
        };
        if let Some(index) = self
            .interner
            .marks
            .iter()
            .position(|known| *known == record)
        {
            return index as u32;
        }
        self.interner.marks.push(record);
        self.w.marks.push(record);
        (self.w.marks.len() - 1) as u32
    }

    fn array(&mut self, items: &[Value], enums: &[Identifier]) -> u32 {
        let values: Vec<image::Value> = items.iter().map(|item| self.value(item, enums)).collect();
        if let Some((_, index)) = self
            .interner
            .arrays
            .iter()
            .find(|(range, _)| range.get(&self.w.values) == values)
        {
            return *index;
        }
        let range = push(&mut self.w.values, values);
        let index = self.w.arrays.len() as u32;
        self.w.arrays.push(image::Array { items: range });
        self.interner.arrays.push((range, index));
        index
    }

    fn value(&mut self, value: &Value, enums: &[Identifier]) -> image::Value {
        let (kind, word) = match value {
            Value::Void => (image::VALUE_VOID, 0),
            Value::Int(value) => (image::VALUE_INT, *value as u32),
            Value::Float(value) => (image::VALUE_FLOAT, value.to_bits()),
            Value::Bool(value) => (image::VALUE_BOOL, u32::from(*value)),
            Value::Color(color) => (image::VALUE_COLOR, color_word(*color)),
            Value::Enum(name) => (image::VALUE_ENUM, enum_index(enums, name)),
            Value::Marks(marks) => (image::VALUE_MARKS, self.marks(marks)),
            Value::Curve(curve) => (image::VALUE_CURVE, self.curve(curve)),
            Value::Gradient(gradient) => (image::VALUE_GRADIENT, self.gradient(gradient)),
            Value::Array(items) => (image::VALUE_ARRAY, self.array(items, enums)),
        };
        image::Value { kind, word }
    }

    /// A resource parameter's or constant's record.
    fn resource(&mut self, value: &Value, enums: &[Identifier]) -> image::ResourceRef {
        let (kind, index) = match value {
            Value::Curve(curve) => (image::RESOURCE_CURVE, self.curve(curve)),
            Value::Gradient(gradient) => (image::RESOURCE_GRADIENT, self.gradient(gradient)),
            Value::Marks(marks) => (image::RESOURCE_MARKS, self.marks(marks)),
            Value::Array(items) => (image::RESOURCE_ARRAY, self.array(items, enums)),
            _ => unreachable!("resource parameters hold resources"),
        };
        image::ResourceRef { kind, index }
    }

    /// Write `program`, an effect's or, with its input count, an operator's.
    fn program(&mut self, program: &BytecodeProgram, inputs: Option<usize>) -> u32 {
        let code = push(
            &mut self.w.code,
            program.code.iter().map(InstructionWord::encode),
        );
        let params = push(
            &mut self.w.params,
            program
                .params
                .iter()
                .enumerate()
                .map(|(index, ty)| image::Param {
                    kind: ParameterKind::for_type(ty).to_wire(),
                    bank: u32::from(param_bank(&program.params, index)),
                }),
        );
        let operands = push(&mut self.w.operands, program.operands.iter().copied());
        let curves: Vec<_> = program
            .curves
            .iter()
            .map(|curve| image::ResourceRef {
                kind: image::RESOURCE_CURVE,
                index: self.curve(curve),
            })
            .collect();
        let gradients: Vec<_> = program
            .gradients
            .iter()
            .map(|gradient| image::ResourceRef {
                kind: image::RESOURCE_GRADIENT,
                index: self.gradient(gradient),
            })
            .collect();
        let marks: Vec<_> = program
            .marks
            .iter()
            .map(|marks| image::ResourceRef {
                kind: image::RESOURCE_MARKS,
                index: self.marks(marks),
            })
            .collect();
        let arrays: Vec<_> = program
            .arrays
            .iter()
            .map(|items| image::ResourceRef {
                kind: image::RESOURCE_ARRAY,
                index: self.array(items, &program.enums),
            })
            .collect();
        let resources = &mut self.w.program_resources;
        let (curves, gradients, marks, arrays) = (
            push(resources, curves),
            push(resources, gradients),
            push(resources, marks),
            push(resources, arrays),
        );
        let stencil = program
            .body()
            .iter()
            .any(|instruction| matches!(instruction, Instruction::Stencil { .. }));
        let flags = [
            (inputs.is_some(), image::PROGRAM_OPERATOR),
            (program.uses_progress(), image::PROGRAM_USES_PROGRESS),
            (program.uses_spatial_context(), image::PROGRAM_USES_SPATIAL),
            (program.uses_sections(), image::PROGRAM_USES_SECTIONS),
            (program.reads_target(), image::PROGRAM_READS_TARGET),
            (
                program.uses_pixel_context(),
                image::PROGRAM_USES_PIXEL_CONTEXT,
            ),
            (stencil, image::PROGRAM_HAS_STENCIL),
        ]
        .iter()
        .filter(|(set, _)| *set)
        .fold(0, |flags, (_, flag)| flags | flag);
        self.w.programs.push(image::Program {
            code,
            query_end: u32::from(program.query_end),
            target_end: u32::from(program.target_end),
            result: u32::from(program.result.bits()),
            depth: u32::from(program.depth),
            scalars: counts(program.scalars),
            rows: counts(program.rows),
            params,
            curves,
            gradients,
            marks,
            arrays,
            operands,
            frame_caches: u32::from(program.frame_caches),
            inputs: inputs.unwrap_or(0) as u32,
            flags,
        });
        (self.w.programs.len() - 1) as u32
    }

    /// Write an invocation's checked values for `program`.
    fn param_set(
        &mut self,
        program: &BytecodeProgram,
        params: &donder_runtime_types::BoundParams,
    ) -> u32 {
        let mut words = Vec::new();
        let mut resources = Vec::new();
        for (ty, value) in program.params.iter().zip(params.values()) {
            match ParameterKind::for_type(ty).storage() {
                None => {}
                Some(ParamStorage::Word) => words.push(match (ty, value) {
                    (Type::Float, Value::Int(value)) => (*value as f32).to_bits(),
                    (_, Value::Float(value)) => value.to_bits(),
                    (_, Value::Int(value)) => *value as u32,
                    (_, Value::Bool(value)) => u32::from(*value),
                    (_, Value::Color(color)) => color_word(*color),
                    (_, Value::Enum(name)) => enum_index(&program.enums, name),
                    _ => unreachable!("word parameters hold scalar values"),
                }),
                Some(ParamStorage::Resource) => {
                    resources.push(self.resource(value, &program.enums));
                }
            }
        }
        let words = push(&mut self.w.words, words);
        let resources = push(&mut self.w.resources, resources);
        self.w.param_sets.push(image::ParamSet { words, resources });
        (self.w.param_sets.len() - 1) as u32
    }

    /// Write automation already checked with the invocation of `program`;
    /// [`NONE`] without bindings.
    fn automation(
        &mut self,
        program: &BytecodeProgram,
        bindings: &[PreparedAutomation],
        origin: SampleTime,
    ) -> u32 {
        if bindings.is_empty() {
            return NONE;
        }
        let mut records = Vec::with_capacity(bindings.len());
        let mut windows: Vec<image::Window> = Vec::new();
        for binding in bindings {
            let bank = u32::from(param_bank(
                &program.params,
                usize::from(binding.param_index),
            ));
            let (mut min, mut max, mut options) = (0, 0, Range::EMPTY);
            let (kind, destination) = match (&binding.mapping, binding.quantity) {
                (
                    AutomationMapping::Float {
                        min: low,
                        max: high,
                    },
                    quantity,
                ) => {
                    (min, max) = (low.to_bits(), high.to_bits());
                    let kind = if quantity == AutomatedQuantity::Integral {
                        image::BINDING_FLOAT_INTEGRAL
                    } else {
                        image::BINDING_FLOAT
                    };
                    (kind, bank)
                }
                (_, AutomatedQuantity::Integral) => {
                    unreachable!("admission accepts integrals of float automation only")
                }
                (
                    AutomationMapping::Int {
                        min: low,
                        max: high,
                    },
                    _,
                ) => {
                    (min, max) = (*low as u32, *high as u32);
                    (image::BINDING_INT, bank)
                }
                (AutomationMapping::Bool, _) => (image::BINDING_BOOL, bank),
                (AutomationMapping::Enum { values }, _) => {
                    // Lowering names every option of a bound enum.
                    options = push(
                        &mut self.w.enum_options,
                        values.iter().map(|value| enum_index(&program.enums, value)),
                    );
                    (image::BINDING_ENUM, bank)
                }
                (
                    AutomationMapping::Curve {
                        min: low,
                        max: high,
                    },
                    _,
                ) => {
                    (min, max) = (low.to_bits(), high.to_bits());
                    let points = binding.curve.points.len().max(1) as u32;
                    let window = match windows.iter().position(|window| window.resource == bank) {
                        Some(index) => {
                            windows[index].points = windows[index].points.max(points);
                            index
                        }
                        None => {
                            windows.push(image::Window {
                                resource: bank,
                                points,
                            });
                            windows.len() - 1
                        }
                    };
                    (image::BINDING_CURVE, window as u32)
                }
            };
            records.push(image::Binding {
                parameter: u32::from(binding.param_index),
                start: binding.start.as_ticks(),
                duration: binding.duration.as_ticks(),
                curve: self.curve(&binding.curve),
                kind,
                destination,
                min,
                max,
                options,
            });
        }
        let bindings = push(&mut self.w.bindings, records);
        let windows = push(&mut self.w.windows, windows);
        self.w.automation.push(image::Automation {
            bindings,
            windows,
            origin: origin.as_ticks(),
        });
        (self.w.automation.len() - 1) as u32
    }
}

impl<'id> SequenceBuilder<'id> {
    pub(crate) fn new(timing: SequenceTiming) -> Self {
        Self {
            timing,
            w: SectionsWriter::default(),
            interner: Interner::default(),
            fixtures: Vec::new(),
            geometry: Vec::new(),
            storage: Vec::new(),
            pixel_count: 0,
            targets: Vec::new(),
            sample_programs: Vec::new(),
            operator_programs: Vec::new(),
            clips: Vec::new(),
            layers: Vec::new(),
            nodes: Vec::new(),
            outputs: Vec::new(),
            collections: Vec::new(),
            controls: Vec::new(),
            brand: PhantomData,
        }
    }

    pub fn windows(&self) -> impl ExactSizeIterator<Item = WindowHandle<'id>> + '_ {
        self.timing.windows().iter().map(|window| WindowHandle {
            start: window.start,
            duration: SampleDuration::from_ticks(window.duration.get()),
            brand: PhantomData,
        })
    }

    pub fn whole_sequence(&self) -> WindowHandle<'id> {
        WindowHandle {
            start: SampleTime::from_ticks(0),
            duration: self.timing.duration(),
            brand: PhantomData,
        }
    }

    /// Register original geometry and its already selected storage cells together.
    pub fn fixture(&mut self, source_id: u32, geometry: FixtureGeometry) -> FixtureHandle<'id> {
        let index = self.geometry.len();
        let pixel_count = geometry.cells().len();
        self.storage.push(self.fixtures.len());
        if geometry.retains_fixture() {
            self.fixtures.push(image::Fixture {
                id: source_id,
                pixel_count: pixel_count as u32,
                offset: self.pixel_count as u32,
                positions: Range::EMPTY,
            });
            self.pixel_count += pixel_count;
        }
        self.geometry.push(geometry);
        FixtureHandle::new(index)
    }

    /// Geometry and storage are derived from the same owner-issued handles.
    /// The language helper supplies original sampling coordinates and sections.
    pub fn target(
        &mut self,
        fixtures: impl IntoIterator<Item = FixtureHandle<'id>>,
        scope: TargetScope,
    ) -> TargetHandle<'id> {
        let domain = TargetGeometry::new(
            fixtures
                .into_iter()
                .map(|fixture| (fixture.index, &self.geometry[fixture.index])),
            scope,
        );
        let mut pixels = Vec::with_capacity(domain.pixels().len());
        let mut spatial = Vec::with_capacity(domain.pixels().len());
        let mut source_pixels = Vec::with_capacity(domain.pixels().len());
        for pixel in domain.pixels() {
            // The domain emits only registered retained cells. Empty fixtures
            // contribute original coordinates/sections but never storage addresses.
            pixels.push(PreparedPixel {
                fixture_index: self.storage[pixel.fixture()],
                fixture_pixel_index: pixel.cell(),
                pixel_index: pixel.index(),
                pixel_count: pixel.count(),
            });
            spatial.push(pixel.spatial());
            source_pixels.push((pixel.fixture(), pixel.source_cell()));
        }
        self.store_target(Target {
            pixels,
            spatial,
            source_pixels,
            selection: domain.selection().into(),
            scope,
        })
    }

    fn store_target(&mut self, target: Target) -> TargetHandle<'id> {
        if let Some(index) = self.targets.iter().position(|existing| {
            existing.pixels == target.pixels
                && existing.spatial == target.spatial
                && existing.source_pixels == target.source_pixels
                && existing.selection == target.selection
                && existing.scope == target.scope
        }) {
            return TargetHandle::new(index);
        }
        self.targets.push(target);
        TargetHandle::new(self.targets.len() - 1)
    }

    pub fn target_pixel_count(&self, target: TargetHandle<'id>) -> usize {
        self.targets[target.index].pixels.len()
    }

    /// Intersect a range with the target's physical pixel order. Out-of-domain
    /// endpoints and reversed ranges select no extra pixels. Logical indices,
    /// counts, fractions, spatial scope and selection order survive.
    pub fn target_slice(
        &mut self,
        target: TargetHandle<'id>,
        range: core::ops::Range<usize>,
    ) -> TargetHandle<'id> {
        let original = &self.targets[target.index];
        let pixels = keep(&original.pixels, &range);
        let spatial = keep(&original.spatial, &range);
        let source_pixels = keep(&original.source_pixels, &range);
        let selected: BTreeSet<_> = source_pixels.iter().copied().collect();
        let selection = original
            .selection
            .iter()
            .filter(|source| selected.contains(source))
            .copied()
            .collect();
        let scope = original.scope;
        self.store_target(Target {
            pixels,
            spatial,
            source_pixels,
            selection,
            scope,
        })
    }

    pub fn sample(
        &mut self,
        invocation: &SampleInvocation,
        window: WindowHandle<'id>,
        target: TargetHandle<'id>,
    ) -> EffectHandle<'id> {
        let bytecode = invocation.program().bytecode();
        let program = match self
            .sample_programs
            .iter()
            .find(|(program, _)| Shared::ptr_eq(program, invocation.program()))
        {
            Some((_, index)) => *index,
            None => {
                let index = self.program(bytecode, None);
                self.sample_programs
                    .push((Shared::clone(invocation.program()), index));
                index
            }
        };
        let params = self.param_set(bytecode, invocation.params());
        let automation = self.automation(bytecode, invocation.automation(), window.start);
        self.w.effects.push(image::Effect {
            start: window.start.as_ticks(),
            duration: window.duration.as_ticks(),
            target: target.index as u32,
            program,
            params,
            automation,
            state: NONE,
        });
        EffectHandle::new(self.w.effects.len() - 1)
    }

    /// Make `effect` a rig voice that commands start and stop; returns its
    /// index in the rig's voices.
    pub fn voice(&mut self, effect: EffectHandle<'id>, length: VoiceLength) -> u16 {
        self.w.voices.push(image::Voice {
            effect: effect.index as u32,
            duration: match length {
                VoiceLength::Fixed(duration) => duration.as_ticks(),
                VoiceLength::Open => NONE,
            },
        });
        (self.w.voices.len() - 1) as u16
    }

    /// A live mark collection read by the given marks parameters of effects
    /// (their program's parameter indices); returns its index.
    pub fn live_marks(
        &mut self,
        readers: impl IntoIterator<Item = (EffectHandle<'id>, u16)>,
    ) -> u16 {
        let readers = readers
            .into_iter()
            .map(|(effect, param)| {
                let record = &self.w.effects[effect.index];
                let program = &self.w.programs[record.program as usize];
                let param = &program.params.get(&self.w.params)[usize::from(param)];
                image::Reader {
                    effect: effect.index as u32,
                    bank: param.bank,
                }
            })
            .collect();
        self.collections.push(readers);
        (self.collections.len() - 1) as u16
    }

    /// A control that sets the given automation bindings of effects (indices
    /// in each effect's bindings); returns its index.
    pub fn control(&mut self, targets: impl IntoIterator<Item = (EffectHandle<'id>, u16)>) -> u16 {
        let targets = targets
            .into_iter()
            .map(|(effect, binding)| image::ControlTarget {
                effect: effect.index as u32,
                binding: u32::from(binding),
            })
            .collect();
        self.controls.push(targets);
        (self.controls.len() - 1) as u16
    }

    /// Register authored clip identity independently from storage order.
    pub fn clip(&mut self, id: u32, effect: EffectHandle<'id>) {
        self.clips.push(image::Clip {
            id,
            effect: effect.index as u32,
        });
    }

    /// Request each declared input exactly once. References can only point to
    /// signals already created by this builder, so cycles cannot be expressed.
    pub fn operator(
        &mut self,
        invocation: &OperatorInvocation,
        mut input: impl FnMut(usize) -> SignalHandle<'id>,
    ) -> SignalHandle<'id> {
        let bytecode = invocation.program().bytecode();
        let program = match self
            .operator_programs
            .iter()
            .find(|(program, _)| Shared::ptr_eq(program, invocation.program()))
        {
            Some((_, index)) => *index,
            None => {
                let inputs = invocation.program().input_count();
                let index = self.program(bytecode, Some(inputs));
                self.operator_programs
                    .push((Shared::clone(invocation.program()), index));
                index
            }
        };
        let params = self.param_set(bytecode, invocation.params());
        let automation =
            self.automation(bytecode, invocation.automation(), SampleTime::from_ticks(0));
        let inputs = (0..invocation.program().input_count())
            .map(|index| input(index).index as u32)
            .collect();
        self.nodes.push(Node {
            kind: image::NODE_OPERATOR,
            layer: 0,
            program,
            params,
            inputs,
            automation,
        });
        SignalHandle::new(self.nodes.len() - 1)
    }

    pub fn layer(
        &mut self,
        enabled: bool,
        effects: impl IntoIterator<Item = EffectHandle<'id>>,
    ) -> SignalHandle<'id> {
        let mut effects: Vec<u32> = effects
            .into_iter()
            .map(|effect| effect.index as u32)
            .collect();
        effects.sort_by_key(|&index| self.w.effects[index as usize].start);
        self.layers.push(Layer { enabled, effects });
        self.nodes.push(Node {
            kind: image::NODE_LAYER,
            layer: (self.layers.len() - 1) as u32,
            program: 0,
            params: 0,
            inputs: Vec::new(),
            automation: NONE,
        });
        SignalHandle::new(self.nodes.len() - 1)
    }

    /// The channel-wise maximum of `inputs`: how several sources fanning
    /// into one port combine.
    pub fn mix(
        &mut self,
        inputs: impl IntoIterator<Item = SignalHandle<'id>>,
    ) -> SignalHandle<'id> {
        self.nodes.push(Node {
            kind: image::NODE_MIX,
            layer: 0,
            program: 0,
            params: 0,
            inputs: inputs.into_iter().map(|input| input.index as u32).collect(),
            automation: NONE,
        });
        SignalHandle::new(self.nodes.len() - 1)
    }

    /// The signal the show displays; none is black.
    pub fn output(&mut self, signal: Option<SignalHandle<'id>>) -> SequenceRoot<'id> {
        SequenceRoot(signal)
    }

    pub fn port(&mut self, controller_index: u32, port: u32) -> OutputHandle<'id> {
        self.outputs.push(Output {
            controller_index,
            port,
            bytes: 0,
        });
        OutputHandle::new(self.outputs.len() - 1)
    }

    pub fn lookup(&mut self, table: [u8; 256]) -> LookupHandle<'id> {
        self.w.lookups.push(image::Lookup { table });
        LookupHandle::new(self.w.lookups.len() - 1)
    }

    pub fn padding(&mut self, output: OutputHandle<'id>, bytes: usize) {
        self.outputs[output.index].bytes += bytes;
    }

    /// Append the target's physical pixel order. Output width and all byte spans
    /// follow from storage actually reserved here, never caller-supplied offsets.
    pub fn route(
        &mut self,
        output: OutputHandle<'id>,
        target: TargetHandle<'id>,
        encoding: OutputEncoding,
        lookup: Option<LookupHandle<'id>>,
    ) {
        let (rgbw, order) = match encoding.encoding() {
            PixelEncoding::Rgb { order } => (false, [order[0], order[1], order[2], 0]),
            PixelEncoding::Rgbw { order } => (true, order),
        };
        let channels = if rgbw { 4 } else { 3 };
        let pixels = &self.targets[target.index].pixels;
        let mut start_slot = self.outputs[output.index].bytes;
        self.outputs[output.index].bytes += pixels.len() * channels;
        let mut indices = pixels
            .iter()
            .map(|pixel| {
                self.fixtures[pixel.fixture_index].offset as usize
                    + pixel.fixture_pixel_index as usize
            })
            .peekable();
        while let Some(start) = indices.next() {
            let mut end = start + 1;
            while indices.peek() == Some(&end) {
                indices.next();
                end += 1;
            }
            self.w.routes.push(image::Route {
                first: start as u32,
                end: end as u32,
                output: output.index as u32,
                start_slot: start_slot as u32,
                encoding: if rgbw {
                    image::ENCODING_RGBW
                } else {
                    image::ENCODING_RGB
                },
                order,
                lookup: lookup.map_or(NONE, |lookup| lookup.index as u32),
            });
            start_slot += (end - start) * channels;
        }
    }

    /// The image: every record written, then the plan, layers, nodes and
    /// targets that the registered handles describe.
    pub(crate) fn finish(mut self, root: SequenceRoot<'id>) -> alloc::boxed::Box<[u64]> {
        let full = self.target(
            (0..self.geometry.len())
                .map(FixtureHandle::new)
                .collect::<Vec<_>>(),
            TargetScope::PerFixture,
        );
        let mut needs_spatial = vec![false; self.targets.len()];
        let mut needs_sections = vec![false; self.targets.len()];
        for effect in &self.w.effects {
            let flags = self.w.programs[effect.program as usize].flags;
            needs_spatial[effect.target as usize] |= flags & image::PROGRAM_USES_SPATIAL != 0;
            needs_sections[effect.target as usize] |= flags & image::PROGRAM_USES_SECTIONS != 0;
        }
        for (_, program) in &self.operator_programs {
            let flags = self.w.programs[*program as usize].flags;
            needs_spatial[full.index] |= flags & image::PROGRAM_USES_SPATIAL != 0;
            needs_sections[full.index] |= flags & image::PROGRAM_USES_SECTIONS != 0;
        }
        let mut fixture_spatial = vec![false; self.fixtures.len()];
        for (target, &spatial) in self.targets.iter().zip(&needs_spatial) {
            if spatial {
                for pixel in &target.pixels {
                    fixture_spatial[pixel.fixture_index] = true;
                }
            }
        }
        let w = &mut self.w;
        for (index, geometry) in self.geometry.iter().enumerate() {
            let fixture = self.storage[index];
            if geometry.retains_fixture() && fixture_spatial[fixture] {
                self.fixtures[fixture].positions = push(
                    &mut w.positions,
                    geometry.cells().iter().map(|&cell| {
                        let [x, y] = geometry.positions()[cell as usize];
                        image::Position { x, y }
                    }),
                );
            }
        }
        w.fixtures = core::mem::take(&mut self.fixtures);
        // Only effects and the plan read targets; routes were resolved when
        // they were added, so targets used only by routes are not written.
        let mut written = vec![NONE; self.targets.len()];
        written[full.index] = 0;
        for effect in &w.effects {
            written[effect.target as usize] = 0;
        }
        let (mut runs, mut indexed) = (Vec::new(), Vec::new());
        for (index, target) in self.targets.iter().enumerate() {
            if written[index] == NONE {
                continue;
            }
            written[index] = w.targets.len() as u32;
            let record = write_target(
                w,
                &mut runs,
                &mut indexed,
                target,
                needs_spatial[index],
                needs_sections[index],
            );
            w.targets.push(record);
        }
        for effect in &mut w.effects {
            effect.target = written[effect.target as usize];
        }
        for layer in &self.layers {
            let effects = push(&mut w.layer_order, layer.effects.iter().copied());
            let mut latest = 0u32;
            let ends: Vec<u32> = layer
                .effects
                .iter()
                .map(|&effect| {
                    let effect = &w.effects[effect as usize];
                    latest = latest.max(effect.start.saturating_add(effect.duration));
                    latest
                })
                .collect();
            w.layer_ends.extend(ends);
            w.layers.push(image::Layer {
                enabled: u32::from(layer.enabled),
                effects,
            });
        }
        // Each operator's depth slot is the deepest of its inputs; the slots
        // in use are the deepest operator's.
        let mut depths = Vec::with_capacity(self.nodes.len());
        let mut vm_slots = 0;
        for node in &self.nodes {
            let deepest = node
                .inputs
                .iter()
                .fold(0, |depth, &input| depth.max(depths[input as usize]));
            let (vm_slot, depth) = match node.kind {
                image::NODE_OPERATOR => (deepest, deepest + 1),
                _ => (0, deepest),
            };
            vm_slots = vm_slots.max(depth);
            depths.push(depth);
            let inputs = push(&mut w.node_inputs, node.inputs.iter().copied());
            w.nodes.push(image::Node {
                kind: node.kind,
                layer: node.layer,
                program: node.program,
                params: node.params,
                inputs,
                automation: node.automation,
                vm_slot,
            });
        }
        let mut offset = 0;
        for output in &self.outputs {
            w.outputs.push(image::Output {
                controller_index: output.controller_index,
                port: output.port,
                offset,
                width: output.bytes as u32,
            });
            offset += output.bytes as u32;
        }
        let mut next = 0;
        for (index, effect) in w.effects.iter_mut().enumerate() {
            let read = self
                .collections
                .iter()
                .flatten()
                .any(|reader| reader.effect as usize == index);
            if effect.automation != NONE || read {
                effect.state = next;
                next += 1;
            }
        }
        for readers in &self.collections {
            let readers = push(&mut w.readers, readers.iter().copied());
            w.collections.push(image::Collection { readers });
        }
        for targets in &self.controls {
            let targets = push(&mut w.control_targets, targets.iter().copied());
            w.controls.push(image::Control { targets });
        }
        self.clips.sort_unstable_by_key(|clip| clip.id);
        w.clips = core::mem::take(&mut self.clips);
        w.meta.push(image::Meta {
            frame_rate: self.timing.frame_rate(),
            frame_count: self.timing.frame_count(),
            duration: self.timing.duration().as_ticks(),
            pixel_count: self.pixel_count as u32,
            plan_target: written[full.index],
            output: root.0.map_or(NONE, |signal| signal.index as u32),
            vm_slots,
        });
        w.finish(crate::show::FORMAT_VERSION)
    }
}

/// Write a target's pixels, the smaller of runs and one record per pixel,
/// sharing equal lists, with its scope bounds and section places.
fn write_target(
    w: &mut SectionsWriter,
    known_runs: &mut Vec<Range>,
    known_pixels: &mut Vec<Range>,
    target: &Target,
    spatial: bool,
    sections: bool,
) -> image::Target {
    let pixels = &target.pixels;
    let mut runs = Vec::new();
    let mut start = 0;
    for index in 1..=pixels.len() {
        if index < pixels.len() {
            let (before, next) = (&pixels[index - 1], &pixels[index]);
            if before.fixture_index == next.fixture_index
                && before.fixture_pixel_index.checked_add(1) == Some(next.fixture_pixel_index)
                && before.pixel_index.checked_add(1) == Some(next.pixel_index)
                && before.pixel_count == next.pixel_count
            {
                continue;
            }
        }
        let first = &pixels[start];
        runs.push(image::PixelRun {
            end: index as u32,
            fixture: first.fixture_index as u32,
            first_cell: first.fixture_pixel_index,
            first_index: first.pixel_index as u32,
            count: first.pixel_count as u32,
        });
        start = index;
    }
    let (kind, range) =
        if runs.len() * size_of::<image::PixelRun>() >= pixels.len() * size_of::<image::Pixel>() {
            let indexed: Vec<_> = pixels
                .iter()
                .map(|pixel| image::Pixel {
                    fixture: pixel.fixture_index as u32,
                    cell: pixel.fixture_pixel_index,
                    index: pixel.pixel_index as u32,
                    count: pixel.pixel_count as u32,
                })
                .collect();
            (
                image::PIXELS_INDEXED,
                shared(&mut w.pixels, known_pixels, &indexed),
            )
        } else {
            (
                image::PIXELS_RUNS,
                shared(&mut w.pixel_runs, known_runs, &runs),
            )
        };
    let spatial = if spatial {
        let mut runs = Vec::<image::SpatialRun>::new();
        for (index, context) in target.spatial.iter().enumerate() {
            match runs.last_mut() {
                Some(run)
                    if run.min.map(f32::to_bits) == context.min.map(f32::to_bits)
                        && run.max.map(f32::to_bits) == context.max.map(f32::to_bits) =>
                {
                    run.end = index as u32 + 1;
                }
                _ => runs.push(image::SpatialRun {
                    end: index as u32 + 1,
                    min: context.min,
                    max: context.max,
                }),
            }
        }
        push(&mut w.spatial_runs, runs)
    } else {
        Range::EMPTY
    };
    let max_count = pixels
        .iter()
        .fold(0, |count, pixel| count.max(pixel.pixel_count));
    let sample_count = if pixels.len() > max_count {
        max_count
    } else {
        0
    };
    let (section_runs, section_kind, section_pixels) = if sections {
        write_sections(w, &target.selection, &target.source_pixels)
    } else {
        (Range::EMPTY, image::PIXELS_RUNS, Range::EMPTY)
    };
    image::Target {
        kind,
        pixels: range,
        spatial,
        section_runs,
        section_kind,
        section_pixels,
        per_fixture: u32::from(target.scope == TargetScope::PerFixture),
        sample_count: sample_count as u32,
    }
}

/// Section membership from the original selection: its runs of consecutive
/// cells, and each target pixel's place among them, as spans or one record
/// per pixel, whichever is smaller.
fn write_sections(
    w: &mut SectionsWriter,
    selection: &[(usize, u32)],
    pixels: &[(usize, u32)],
) -> (Range, u32, Range) {
    let mut runs = Vec::<image::SectionRun>::new();
    let mut addresses = alloc::collections::BTreeMap::new();
    for &(fixture, index) in selection {
        match runs.last_mut() {
            Some(run)
                if run.fixture == fixture as u32 && run.last.checked_add(1) == Some(index) =>
            {
                run.last = index;
            }
            _ => runs.push(image::SectionRun {
                fixture: fixture as u32,
                first: index,
                last: index,
            }),
        }
        addresses.insert(
            (fixture, index),
            image::SectionPixel {
                run: (runs.len() - 1) as u32,
                index,
            },
        );
    }
    let mut spans = Vec::<image::SectionSpan>::new();
    for (index, pixel) in pixels.iter().enumerate() {
        let address = addresses[pixel];
        let start = if spans.len() > 1 {
            spans[spans.len() - 2].end as usize
        } else {
            0
        };
        match spans.last_mut() {
            Some(span)
                if span.first.run == address.run
                    && span.first.index.checked_add((index - start) as u32)
                        == Some(address.index) =>
            {
                span.end = index as u32 + 1;
            }
            _ => spans.push(image::SectionSpan {
                end: index as u32 + 1,
                first: address,
            }),
        }
    }
    let runs = push(&mut w.section_runs, runs);
    if spans.len() * size_of::<image::SectionSpan>()
        < pixels.len() * size_of::<image::SectionPixel>()
    {
        (runs, image::PIXELS_RUNS, push(&mut w.section_spans, spans))
    } else {
        let places = pixels.iter().map(|pixel| addresses[pixel]);
        (
            runs,
            image::PIXELS_INDEXED,
            push(&mut w.section_pixels, places),
        )
    }
}

/// The items at the indices of `range`.
fn keep<T: Copy>(items: &[T], range: &core::ops::Range<usize>) -> Vec<T> {
    items
        .iter()
        .enumerate()
        .filter(|(index, _)| range.contains(index))
        .map(|(_, item)| *item)
        .collect()
}
