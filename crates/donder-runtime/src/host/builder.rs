//! Invariant-preserving show construction. Checked authored inputs enter before
//! `build`; graph references and storage addresses are issued only by its
//! owner, which writes the image's sections.
use crate::image::{self, NONE, Range, SectionsWriter};
use alloc::{vec, vec::Vec};
use core::marker::PhantomData;
use donder_runtime_types::bytecode::{
    Banks, BytecodeProgram, ParamStorage, ParameterKind, param_bank,
};
use donder_runtime_types::{
    AutomatedQuantity, AutomationMapping, BoundProgram, Curve, FixtureGeometry, Gradient,
    Identifier, Marks, OutputEncoding, PixelEncoding, PreparedAutomation, Program, SampleDuration,
    SampleTime, SequenceTiming, Shared, SpatialContext, TargetGeometry, TargetScope, Type, Value,
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
handle!(TargetHandle, EffectHandle, SignalHandle, OutputHandle);

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

pub struct ShowBuilder<'id> {
    timing: SequenceTiming,
    w: SectionsWriter,
    interner: Interner,
    fixtures: Vec<image::Fixture>,
    geometry: Vec<FixtureGeometry>,
    storage: Vec<usize>,
    pixel_count: usize,
    targets: Vec<Target>,
    programs: Vec<(Shared<Program>, u32)>,
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
        counts: [banks.floats, banks.ints, banks.bools, banks.resources],
    }
}

fn enum_index(enums: &[Identifier], name: &Identifier) -> u32 {
    enums
        .iter()
        .position(|option| option == name)
        .map_or(-1, |index| index as i32) as u32
}

impl ShowBuilder<'_> {
    /// A color's index in the colors section, shared by equal colors.
    fn color(&mut self, color: donder_runtime_types::Color) -> u32 {
        match self.w.colors.iter().position(|known| *known == color) {
            Some(index) => index as u32,
            None => {
                self.w.colors.push(color);
                (self.w.colors.len() - 1) as u32
            }
        }
    }

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
        // An empty curve is missing: one NaN point, so playback never meets
        // an empty one.
        let missing = [donder_runtime_types::CurvePoint {
            position: 0.0,
            value: f32::NAN,
        }];
        let source = if curve.points.is_empty() {
            &missing[..]
        } else {
            &curve.points[..]
        };
        let points = push(&mut w.curve_points, source.iter().copied());
        let mut areas = vec![0.0; source.len()];
        donder_runtime_types::sampling::curve_prefix_areas(source, &mut areas);
        let areas = push(&mut w.curve_areas, areas);
        let mut slopes = vec![0.0; source.len()];
        donder_runtime_types::sampling::curve_slopes(source, &mut slopes);
        let slopes = push(&mut w.curve_slopes, slopes);
        let mut crossings = vec![image::CrossingSegment::default(); source.len()];
        let (kind, len) = crate::resources::curve_crossings(source, &mut crossings);
        let crossings = push(&mut w.crossings, crossings[..len].iter().copied());
        let index = w.curves.len() as u32;
        w.curves.push(image::Curve {
            points,
            areas,
            slopes,
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
        // An empty gradient is black: one black stop, so playback never
        // meets an empty one.
        let black = [image::GradientStop {
            position: 0.0,
            color: donder_runtime_types::Color::BLACK,
            inverse: 0.0,
        }];
        let stops: Vec<_> = gradient
            .stops
            .iter()
            .enumerate()
            .map(|(index, stop)| image::GradientStop {
                position: stop.position,
                color: stop.color,
                inverse: gradient.stops.get(index + 1).map_or(0.0, |next| {
                    donder_runtime_types::sampling::Stop::inverse(stop, next)
                }),
            })
            .collect();
        let stops = push(
            &mut w.gradient_stops,
            if stops.is_empty() {
                &black[..]
            } else {
                &stops[..]
            }
            .iter()
            .copied(),
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
            Value::Color(color) => (image::VALUE_COLOR, self.color(*color)),
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

    /// `shared`'s index, written once for equal programs.
    fn program(&mut self, shared: &Shared<Program>) -> u32 {
        if let Some((_, index)) = self
            .programs
            .iter()
            .find(|(existing, _)| Shared::ptr_eq(existing, shared))
            .or_else(|| {
                self.programs
                    .iter()
                    .find(|(existing, _)| **existing == **shared)
            })
        {
            return *index;
        }
        let program = &shared.bytecode;
        let code = push(&mut self.w.code, program.code.iter().copied());
        let resources: Vec<u32> = program
            .resources
            .iter()
            .map(|value| self.resource(value, &program.enums).index)
            .collect();
        let resources = push(&mut self.w.program_resources, resources);
        let stencil = program.has_stencil();
        let flags = [
            (program.uses_spatial_context(), image::PROGRAM_USES_SPATIAL),
            (program.uses_sections(), image::PROGRAM_USES_SECTIONS),
            (program.reads_target(), image::PROGRAM_READS_TARGET),
            (
                program.uses_pixel_context(),
                image::PROGRAM_USES_PIXEL_CONTEXT,
            ),
            (stencil, image::PROGRAM_HAS_STENCIL),
            (program.reads_state(), image::PROGRAM_READS_STATE),
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
            resources,
            scans: u32::from(program.scans),
            inputs: shared.inputs as u32,
            flags,
        });
        let index = (self.w.programs.len() - 1) as u32;
        self.programs.push((Shared::clone(shared), index));
        index
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
                    (_, Value::Color(color)) => self.color(*color),
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
    fn automation(&mut self, program: &BytecodeProgram, bindings: &[PreparedAutomation]) -> u32 {
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
                    unreachable!("the compiler accepts integrals of float automation only")
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
        self.w
            .automation
            .push(image::Automation { bindings, windows });
        (self.w.automation.len() - 1) as u32
    }
}

impl<'id> ShowBuilder<'id> {
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
            programs: Vec::new(),
            clips: Vec::new(),
            layers: Vec::new(),
            nodes: Vec::new(),
            outputs: Vec::new(),
            collections: Vec::new(),
            controls: Vec::new(),
            brand: PhantomData,
        }
    }

    /// Register a fixture's original geometry and the storage cells the show
    /// keeps. Targets name fixtures by the order they are registered in.
    pub fn fixture(&mut self, source_id: u32, geometry: FixtureGeometry) {
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
    }

    /// The pixels of `domain` the show keeps. `domain` describes registered
    /// fixtures' original geometry, by registration order; its logical
    /// indices, counts, spatial scope and selection order survive.
    pub fn target(&mut self, domain: &TargetGeometry) -> TargetHandle<'id> {
        let mut pixels = Vec::with_capacity(domain.pixels().len());
        let mut spatial = Vec::with_capacity(domain.pixels().len());
        let mut source_pixels = Vec::with_capacity(domain.pixels().len());
        for pixel in domain.pixels() {
            let fixture = pixel.fixture();
            // Cells the show does not keep contribute original coordinates
            // and sections but never storage addresses.
            let Ok(cell) = self.geometry[fixture]
                .cells()
                .binary_search(&pixel.source_cell())
            else {
                continue;
            };
            pixels.push(PreparedPixel {
                fixture_index: self.storage[fixture],
                fixture_pixel_index: cell as u32,
                pixel_index: pixel.index(),
                pixel_count: pixel.count(),
            });
            spatial.push(pixel.spatial());
            source_pixels.push((fixture, pixel.source_cell()));
        }
        self.store_target(Target {
            pixels,
            spatial,
            source_pixels,
            selection: domain.selection().into(),
            scope: domain.scope(),
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

    /// An effect: `program` on `target` for `duration` from `start`, within
    /// the show. A sequence's clips carry their authored `id`; a rig's
    /// effects have none.
    pub fn clip(
        &mut self,
        id: Option<u32>,
        program: &BoundProgram,
        start: SampleTime,
        duration: SampleDuration,
        target: TargetHandle<'id>,
    ) -> EffectHandle<'id> {
        assert!(
            start
                .as_ticks()
                .checked_add(duration.as_ticks())
                .is_some_and(|end| end <= self.timing.duration().as_ticks()),
            "an effect plays within its show"
        );
        let bytecode = &program.program().bytecode;
        let index = self.program(program.program());
        let params = self.param_set(bytecode, program.params());
        let automation = self.automation(bytecode, program.automation());
        self.w.effects.push(image::Effect {
            start: start.as_ticks(),
            duration: duration.as_ticks(),
            target: target.index as u32,
            program: index,
            params,
            automation,
        });
        let effect = self.w.effects.len() - 1;
        if let Some(id) = id {
            self.clips.push(image::Clip {
                id,
                effect: effect as u32,
            });
        }
        EffectHandle::new(effect)
    }

    /// Make `effect` a rig voice that commands start, and that stops on a
    /// command or `duration` after each start; returns its index in the
    /// rig's voices.
    pub fn voice(&mut self, effect: EffectHandle<'id>, duration: Option<SampleDuration>) -> u16 {
        self.w.voices.push(image::Voice {
            effect: effect.index as u32,
            duration: duration.map_or(NONE, SampleDuration::as_ticks),
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
                let index = self.w.effects[effect.index].program;
                let (program, _) = self
                    .programs
                    .iter()
                    .find(|(_, written)| *written == index)
                    .unwrap_or_else(|| unreachable!("effects play written programs"));
                image::Reader {
                    effect: effect.index as u32,
                    bank: u32::from(param_bank(&program.bytecode.params, usize::from(param))),
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

    /// Request each declared input exactly once. References can only point to
    /// signals already created by this builder, so cycles cannot be expressed.
    pub fn operator(
        &mut self,
        program: &BoundProgram,
        mut input: impl FnMut(usize) -> SignalHandle<'id>,
    ) -> SignalHandle<'id> {
        let bytecode = &program.program().bytecode;
        let index = self.program(program.program());
        let params = self.param_set(bytecode, program.params());
        let automation = self.automation(bytecode, program.automation());
        let inputs = (0..program.program().inputs)
            .map(|index| input(index).index as u32)
            .collect();
        self.nodes.push(Node {
            kind: image::NODE_OPERATOR,
            layer: 0,
            program: index,
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

    pub fn port(&mut self, controller_index: u32, port: u32) -> OutputHandle<'id> {
        self.outputs.push(Output {
            controller_index,
            port,
            bytes: 0,
        });
        OutputHandle::new(self.outputs.len() - 1)
    }

    pub fn padding(&mut self, output: OutputHandle<'id>, bytes: usize) {
        self.outputs[output.index].bytes += bytes;
    }

    /// Append the target's physical pixel order, each channel byte through
    /// `lookup` if there is one. Returns the pixels it routes. Output width and all byte spans follow from
    /// storage actually reserved here, never caller-supplied offsets.
    pub fn route(
        &mut self,
        output: OutputHandle<'id>,
        target: TargetHandle<'id>,
        encoding: OutputEncoding,
        lookup: Option<[u8; 256]>,
    ) -> usize {
        if self.targets[target.index].pixels.is_empty() {
            return 0;
        }
        let lookup = lookup.map_or(NONE, |table| {
            match self.w.lookups.iter().position(|known| known.table == table) {
                Some(index) => index as u32,
                None => {
                    self.w.lookups.push(image::Lookup { table });
                    (self.w.lookups.len() - 1) as u32
                }
            }
        });
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
                lookup,
            });
            start_slot += (end - start) * channels;
        }
        pixels.len()
    }

    /// The image: every record written, then the plan, layers, nodes and
    /// targets that the registered handles describe.
    pub(crate) fn finish(mut self, root: Option<SignalHandle<'id>>) -> alloc::boxed::Box<[u32]> {
        let full = self.target(&TargetGeometry::new(
            self.geometry.iter().enumerate(),
            TargetScope::PerFixture,
        ));
        let mut needs_spatial = vec![false; self.targets.len()];
        let mut needs_sections = vec![false; self.targets.len()];
        for effect in &self.w.effects {
            let flags = self.w.programs[effect.program as usize].flags;
            needs_spatial[effect.target as usize] |= flags & image::PROGRAM_USES_SPATIAL != 0;
            needs_sections[effect.target as usize] |= flags & image::PROGRAM_USES_SECTIONS != 0;
        }
        for node in &self.nodes {
            if node.kind != image::NODE_OPERATOR {
                continue;
            }
            let flags = self.w.programs[node.program as usize].flags;
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
        // A mix holds every input until it runs; chained, each input joins
        // the running maximum once it is ready, so a mix holds two frames.
        let root = root.map(|signal| chain_mixes(&mut self.nodes, signal.index));
        // Only the nodes the output reads are written, inputs first as they
        // were built. Each writes a frame buffer free from its start until its
        // last reader runs; one that reads its previous frame keeps a state.
        let mut live = vec![false; self.nodes.len()];
        if let Some(root) = root {
            live[root] = true;
        }
        for index in (0..self.nodes.len()).rev() {
            if live[index] {
                for &input in &self.nodes[index].inputs {
                    live[input as usize] = true;
                }
            }
        }
        let mut node_index = vec![NONE; self.nodes.len()];
        let mut last_read = vec![usize::MAX; self.nodes.len()];
        for (index, node) in self.nodes.iter().enumerate() {
            if live[index] {
                for &input in &node.inputs {
                    last_read[input as usize] = index;
                }
            }
        }
        let reads_state =
            |program: u32| w.programs[program as usize].flags & image::PROGRAM_READS_STATE != 0;
        let (mut free, mut buffers, mut states) = (Vec::<u32>::new(), 0, 0);
        for (index, node) in self.nodes.iter().enumerate() {
            if !live[index] {
                continue;
            }
            let buffer = free.pop().unwrap_or_else(|| {
                buffers += 1;
                buffers - 1
            });
            let stateful = match node.kind {
                image::NODE_LAYER => self.layers[node.layer as usize]
                    .effects
                    .iter()
                    .any(|&effect| reads_state(w.effects[effect as usize].program)),
                image::NODE_OPERATOR => reads_state(node.program),
                _ => false,
            };
            let state = if stateful {
                states += 1;
                states - 1
            } else {
                NONE
            };
            for &input in &node.inputs {
                if last_read[input as usize] == index {
                    let input = &w.nodes[node_index[input as usize] as usize];
                    if !free.contains(&input.buffer) {
                        free.push(input.buffer);
                    }
                }
            }
            node_index[index] = w.nodes.len() as u32;
            let inputs = push(
                &mut w.node_inputs,
                node.inputs.iter().map(|&input| node_index[input as usize]),
            );
            w.nodes.push(image::Node {
                kind: node.kind,
                layer: node.layer,
                program: node.program,
                params: node.params,
                inputs,
                automation: node.automation,
                buffer,
                state,
            });
        }
        let buffers = buffers.max(1);
        assert!(
            buffers as usize <= crate::workspace::MAX_BUFFERS,
            "a show's graph holds at most {} frames at once",
            crate::workspace::MAX_BUFFERS
        );
        assert!(
            states as usize <= crate::workspace::MAX_STATES,
            "at most {} signals of a show read their previous frame",
            crate::workspace::MAX_STATES
        );
        assert!(
            w.programs
                .iter()
                .all(|program| program.scans as usize <= crate::workspace::MAX_SCANS),
            "a program runs at most {} scans",
            crate::workspace::MAX_SCANS
        );
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
            output: root.map_or(NONE, |root| node_index[root]),
            buffers,
            states,
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

/// Replace each mix of `nodes` by a chain of two-input mixes, each placed
/// right after the input it adds, inputs first as before. Returns where
/// node `root` went.
fn chain_mixes(nodes: &mut Vec<Node>, root: usize) -> usize {
    let mut readers = vec![Vec::<usize>::new(); nodes.len()];
    for (index, node) in nodes.iter().enumerate() {
        if node.kind == image::NODE_MIX {
            for &input in &node.inputs {
                if !readers[input as usize].contains(&index) {
                    readers[input as usize].push(index);
                }
            }
        }
    }
    let mix = |inputs| Node {
        kind: image::NODE_MIX,
        layer: 0,
        program: 0,
        params: 0,
        inputs,
        automation: NONE,
    };
    let mut chained = Vec::with_capacity(nodes.len());
    let mut moved = vec![0u32; nodes.len()];
    let mut running = vec![None::<u32>; nodes.len()];
    for (index, node) in core::mem::take(nodes).into_iter().enumerate() {
        moved[index] = if node.kind == image::NODE_MIX {
            running[index].unwrap_or_else(|| {
                chained.push(mix(Vec::new()));
                (chained.len() - 1) as u32
            })
        } else {
            let inputs = node
                .inputs
                .iter()
                .map(|&input| moved[input as usize])
                .collect();
            chained.push(Node { inputs, ..node });
            (chained.len() - 1) as u32
        };
        for &reader in &readers[index] {
            running[reader] = Some(match running[reader] {
                None => moved[index],
                Some(sum) => {
                    chained.push(mix(vec![sum, moved[index]]));
                    (chained.len() - 1) as u32
                }
            });
        }
    }
    *nodes = chained;
    moved[root] as usize
}
