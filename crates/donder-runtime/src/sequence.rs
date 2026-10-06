use crate::patch::PreparedPatch;
use crate::signal::{EvaluationWorkspace, PreparedSignalGraph, SignalGraph};
use crate::values::{Color, SampleDuration, SampleTime};
use alloc::{boxed::Box, vec};

mod builder;
pub(crate) mod programs;
mod schedule;
pub use builder::{
    EffectHandle, FixtureHandle, LookupHandle, OutputHandle, SequenceBuilder, SequenceRoot,
    SignalHandle, TargetHandle, WindowHandle,
};
use donder_language::execution::SequenceTiming;
use programs::AdmittedPrograms;

/// Frozen playback data; authoring, elaboration, networking, and pin timing are external.
/// Construction derives valid graph addresses. Archive decoding trusts the
/// compatible producer to preserve those invariants.
#[derive(Clone)]
pub struct PreparedSequence {
    data: ExecutableSequenceData,
}

#[derive(Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct SequenceData<
    P = Box<[crate::dsl::bytecode::BytecodeProgram]>,
    A = Box<[crate::signal::PreparedAutomation]>,
> {
    pub(crate) signals: PreparedSignalGraph<P, A>,
    pub(crate) patch: PreparedPatch,
    pub(crate) outputs: Box<[PreparedOutput]>,
}

pub(crate) type ExecutableSequenceData = SequenceData<AdmittedPrograms, crate::dsl::AutomationPlan>;

/// One output buffer. Controller indices refer to the active setup's controller
/// order; authored document identities and network protocols stay on the host.
#[derive(Clone, Debug, Eq, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct PreparedOutput {
    pub controller_index: u32,
    pub port: u32,
    pub width: usize,
}

/// Owns an admitted sequence, its scratch storage, and its packed output buffers.
pub struct SequencePlayback {
    sequence: PreparedSequence,
    workspace: EvaluationWorkspace,
    outputs: Box<[Box<[u8]>]>,
}

/// Borrowed logical colors and packed controller outputs from one evaluation.
#[derive(Clone, Copy)]
pub struct SequenceFrame<'a> {
    data: &'a ExecutableSequenceData,
    colors: &'a [Color],
    outputs: &'a [Box<[u8]>],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FixtureFrame<'a> {
    pub fixture_id: u32,
    pub pixels: &'a [Color],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OutputFrame<'a> {
    pub output: &'a PreparedOutput,
    pub bytes: &'a [u8],
}

impl<'a> SequenceFrame<'a> {
    pub fn colors(&self) -> &'a [Color] {
        self.colors
    }

    pub fn fixtures(&self) -> impl ExactSizeIterator<Item = FixtureFrame<'a>> + 'a {
        let colors = self.colors;
        self.data
            .signals
            .fixtures
            .iter()
            .zip(&self.data.signals.fixture_pixel_offsets)
            .map(move |(fixture, &offset)| FixtureFrame {
                fixture_id: fixture.id,
                pixels: &colors[offset..offset + fixture.pixel_count],
            })
    }

    pub fn outputs(&self) -> impl ExactSizeIterator<Item = OutputFrame<'a>> + 'a {
        self.data
            .outputs
            .iter()
            .zip(self.outputs)
            .map(|(output, bytes)| OutputFrame { output, bytes })
    }
}

impl PreparedSequence {
    /// Lower accepted inputs through owner-branded handles. All graph addresses,
    /// execution slots, and output spans are derived by the builder.
    pub fn build(
        timing: SequenceTiming,
        build: impl for<'id> FnOnce(&mut SequenceBuilder<'id>) -> SequenceRoot<'id>,
    ) -> Self {
        let mut builder = SequenceBuilder::new(timing);
        let root = build(&mut builder);
        Self::assembled(builder.finish(root))
    }

    pub(crate) fn from_archive(data: SequenceData) -> Result<Self, crate::archive::LoadError> {
        let SequenceData {
            signals,
            patch,
            outputs,
        } = data;
        let signals = programs::restore_graph(signals)?;
        Ok(Self::assembled(SequenceData {
            signals,
            patch,
            outputs,
        }))
    }

    pub(crate) fn assembled(data: ExecutableSequenceData) -> Self {
        Self { data }
    }

    pub(crate) fn archive_data(&self) -> SequenceData {
        SequenceData {
            signals: self.data.signals.to_raw(),
            patch: self.data.patch.clone(),
            outputs: self.data.outputs.clone(),
        }
    }

    fn graph(&self) -> SignalGraph<'_> {
        SignalGraph {
            data: &self.data.signals,
        }
    }

    pub fn fixtures(&self) -> &[crate::signal::PreparedFixture] {
        &self.data.signals.fixtures
    }

    pub fn effect_count(&self) -> usize {
        self.data.signals.effects.len()
    }

    /// Start and duration of each retained effect.
    pub fn effect_windows(
        &self,
    ) -> impl ExactSizeIterator<Item = (SampleTime, SampleDuration)> + '_ {
        self.data
            .signals
            .effects
            .iter()
            .map(|effect| (effect.start_time, effect.duration))
    }

    pub fn active_effect_count(&self, sample_time: SampleTime) -> usize {
        self.data.signals.active_effect_count(sample_time)
    }

    pub fn outputs(&self) -> &[PreparedOutput] {
        &self.data.outputs
    }

    pub fn frame_rate(&self) -> u32 {
        self.data.signals.frame_rate
    }
    pub fn frame_count(&self) -> u32 {
        self.data.signals.frame_count
    }
    pub fn pixel_count(&self) -> usize {
        self.data.signals.pixel_count
    }
    pub fn duration(&self) -> SampleDuration {
        self.data.signals.duration
    }

    /// Resolve an authored clip without loading or preparing its source again.
    pub fn clip(&self, id: u32) -> Option<crate::clip::SequenceClip<'_>> {
        self.data
            .signals
            .clips
            .iter()
            .find(|clip| clip.id == id)
            .map(|clip| crate::clip::SequenceClip {
                graph: self.graph(),
                clip,
            })
    }

    pub fn into_playback(self) -> SequencePlayback {
        let workspace = self.data.signals.create_workspace();
        let outputs = self
            .data
            .outputs
            .iter()
            .map(|output| vec![0; output.width].into_boxed_slice())
            .collect();
        SequencePlayback {
            sequence: self,
            workspace,
            outputs,
        }
    }
}

impl SequencePlayback {
    /// Borrow the packed bytes of the most recently evaluated frame.
    pub fn outputs(&self) -> impl ExactSizeIterator<Item = OutputFrame<'_>> {
        self.sequence
            .data
            .outputs
            .iter()
            .zip(&self.outputs)
            .map(|(output, bytes)| OutputFrame { output, bytes })
    }
    pub fn sequence(&self) -> &PreparedSequence {
        &self.sequence
    }

    pub fn evaluate(&mut self, sample_time: SampleTime) -> SequenceFrame<'_> {
        let data = &self.sequence.data;
        let colors = self
            .sequence
            .graph()
            .evaluate(sample_time, &mut self.workspace);
        data.patch.evaluate(colors, &mut self.outputs);
        SequenceFrame {
            data,
            colors,
            outputs: &self.outputs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::bytecode::{Banks, BytecodeProgram, Instruction, Slot};
    use crate::signal::{PreparedSignalKind, PreparedSignalNode, PreparedTarget, SignalPlan};
    use crate::values::SampleDuration;
    use alloc::vec;
    use alloc::vec::Vec;
    use donder_language::dsl::{OperatorDefinition, SampleDefinition};
    use donder_language::execution::{
        FixtureGeometry, OutputEncoding, RgbOrder, SequenceWindow, TargetScope,
    };

    mod routing;

    /// A resource-free program whose first `prefix` instructions are its
    /// query block and the rest its body.
    pub(super) fn program(
        code: Vec<Instruction>,
        prefix: u16,
        result: Slot,
        scalars: Banks,
        rows: Banks,
    ) -> BytecodeProgram {
        BytecodeProgram {
            code: code.into(),
            query_end: prefix,
            target_end: prefix,
            result,
            scalars,
            rows,
            depth: 0,
            curves: Box::new([]),
            gradients: Box::new([]),
            marks: Box::new([]),
            arrays: Box::new([]),
            enums: Box::new([]),
            operands: Box::new([]),
            frame_caches: 0,
        }
    }

    fn restore_fixture(
        signals: PreparedSignalGraph,
        patch: PreparedPatch,
        outputs: Box<[PreparedOutput]>,
    ) -> Result<PreparedSequence, crate::archive::LoadError> {
        PreparedSequence::from_archive(SequenceData {
            signals,
            patch,
            outputs,
        })
    }

    #[test]
    fn builder_nested_operators_keep_temporal_queries_and_clip_identity() {
        use crate::dsl::{OperatorProgram, SampleProgram};
        use core::num::NonZeroU32;
        let raw = queried_sequence(crate::dsl::bytecode::SignalPixel::Current)
            .archive_data()
            .signals
            .programs;
        let sample =
            SampleDefinition::new(SampleProgram::admit(raw[0].clone(), Box::new([])).unwrap());
        let operator = OperatorDefinition::new(
            OperatorProgram::admit(raw[2].clone(), 1, Box::new([])).unwrap(),
        );
        let sample = sample.bind(vec![]).unwrap();
        let operator = operator.bind(vec![]).unwrap();
        let timing = SequenceTiming::admit(
            NonZeroU32::new(60).unwrap(),
            NonZeroU32::new(61).unwrap(),
            NonZeroU32::new(1_000_000).unwrap(),
            vec![SequenceWindow {
                start: SampleTime::from_ticks(200_000),
                duration: NonZeroU32::new(600_000).unwrap(),
            }]
            .into(),
        )
        .unwrap();
        // Authored nanosecond frame count is preserved independently of the
        // rounded portable duration (e.g. just past one second at 60 Hz).
        assert_eq!(timing.frame_count(), 61);
        assert_eq!(timing.duration().as_ticks(), 1_000_000);
        let sequence = PreparedSequence::build(timing, |builder| {
            let fixture =
                builder.fixture(10, FixtureGeometry::admit(vec![[0.0, 0.0]].into()).unwrap());
            let target = builder.target([fixture], TargetScope::PerFixture);
            let window = builder.windows().next().unwrap();
            let effect = builder.sample(&sample, window, target);
            builder.clip(7, effect);
            let layer = builder.layer(true, [effect]);
            let inner = builder.operator(&operator, |input| {
                assert_eq!(input, 0);
                layer
            });
            let outer = builder.operator(&operator, |input| {
                assert_eq!(input, 0);
                inner
            });
            builder.output([outer])
        });
        assert_eq!(sequence.archive_data().signals.programs.len(), 2);
        let bytes = crate::archive::encode_sequence(&sequence).unwrap();
        let decoded =
            crate::archive::decode_sequence(&bytes, crate::archive::LoadLimits::default()).unwrap();
        let expected = Color {
            red: 17,
            green: 29,
            blue: 43,
        };
        for sequence in [sequence, decoded] {
            let clip = sequence.clip(7).unwrap();
            let mut sampler = clip.sampler(1);
            assert_eq!(
                sampler.evaluate(SampleTime::from_ticks(300_000)),
                [expected]
            );
            assert_eq!(
                sampler.evaluate(SampleTime::from_ticks(900_000)),
                [Color::BLACK]
            );
            drop(sampler);
            let mut playback = sequence.into_playback();
            for ticks in [300_000, 900_000, 0, 500_000] {
                assert_eq!(
                    playback.evaluate(SampleTime::from_ticks(ticks)).colors(),
                    [expected]
                );
            }
        }
    }

    #[test]
    fn builder_derives_storage_routes_and_schedule_from_handles() {
        use core::num::NonZeroU32;
        let raw = timed_sequence().archive_data().signals.programs[0].clone();
        let definition =
            SampleDefinition::new(crate::dsl::SampleProgram::admit(raw, Box::new([])).unwrap());
        let invocation = definition.bind(vec![]).unwrap();
        let timing = SequenceTiming::admit(
            NonZeroU32::new(60).unwrap(),
            NonZeroU32::new(60).unwrap(),
            NonZeroU32::new(1_000_000).unwrap(),
            vec![SequenceWindow {
                start: SampleTime::from_ticks(200_000),
                duration: NonZeroU32::new(600_000).unwrap(),
            }]
            .into(),
        )
        .unwrap();
        let sequence = PreparedSequence::build(timing, |builder| {
            let first = builder.fixture(
                10,
                FixtureGeometry::admit(vec![[0.0, 0.0], [1.0, 0.0]].into()).unwrap(),
            );
            let middle =
                builder.fixture(20, FixtureGeometry::admit(vec![[2.0, 0.0]].into()).unwrap());
            let last =
                builder.fixture(30, FixtureGeometry::admit(vec![[3.0, 0.0]].into()).unwrap());
            let target = builder.target([last, first, first], TargetScope::WholeTarget);
            let unused = builder.target([middle], TargetScope::PerFixture);
            let window = builder.windows().next().unwrap();
            let effect = builder.sample(&invocation, window, target);
            let unreachable = builder.sample(&invocation, window, unused);
            builder.layer(false, [unreachable]);
            let layer = builder.layer(true, [effect]);
            let output = builder.port(3, 7);
            builder.padding(output, 2);
            builder.route(output, target, OutputEncoding::Rgb(RgbOrder::Bgr), None);
            builder.padding(output, 1);
            builder.output([layer])
        });
        assert_eq!(sequence.archive_data().signals.programs.len(), 1);
        assert_eq!(sequence.outputs()[0].width, 12);
        let bytes = crate::archive::encode_sequence(&sequence).unwrap();
        let decoded =
            crate::archive::decode_sequence(&bytes, crate::archive::LoadLimits::default()).unwrap();
        for sequence in [sequence, decoded] {
            let mut playback = sequence.into_playback();
            for ticks in [300_000, 900_000, 300_000, 0] {
                let frame = playback.evaluate(SampleTime::from_ticks(ticks));
                let active = ticks == 300_000;
                let color = if active {
                    Color {
                        red: 17,
                        green: 29,
                        blue: 43,
                    }
                } else {
                    Color::BLACK
                };
                assert_eq!(frame.colors(), [color, color, Color::BLACK, color]);
                let output = frame.outputs().next().unwrap();
                let expected = if active {
                    [0, 0, 43, 29, 17, 43, 29, 17, 43, 29, 17, 0]
                } else {
                    [0; 12]
                };
                assert_eq!(output.bytes, expected);
            }
        }
    }

    fn timed_sequence() -> PreparedSequence {
        use crate::dsl::BoundParams;
        use crate::patch::{PixelEncoding, PreparedPixelRoute};
        use crate::signal::{
            PreparedClip, PreparedEffect, PreparedFixture, PreparedLayer, PreparedPixel,
        };

        let mut data = empty_sequence().archive_data();
        data.signals.fixtures = vec![
            PreparedFixture {
                id: 10,
                pixel_count: 2,
            },
            PreparedFixture {
                id: 20,
                pixel_count: 1,
            },
        ]
        .into();
        data.signals.fixture_pixel_offsets = vec![0, 2].into();
        data.signals.pixel_count = 3;
        data.signals.targets[0].pixels = crate::targets::TargetInterner::default().prepare(vec![
            PreparedPixel::try_new(0, 0, 0, 2, 0.0).unwrap(),
            PreparedPixel::try_new(0, 1, 1, 2, 1.0).unwrap(),
            PreparedPixel::try_new(1, 0, 0, 1, 0.0).unwrap(),
        ]);
        data.signals.programs = vec![program(
            vec![Instruction::ColorConst {
                dst: Slot::scalar(0),
                value: Color {
                    red: 17,
                    green: 29,
                    blue: 43,
                },
            }],
            1,
            Slot::scalar(0),
            Banks {
                colors: 1,
                ..Banks::default()
            },
            Banks::default(),
        )]
        .into();
        data.signals.effects = vec![PreparedEffect {
            start_time: SampleTime::from_ticks(200_000),
            duration: SampleDuration::from_ticks(600_000),
            target: 0,
            program: 0,
            bound_params: BoundParams::default(),
            automation: None,
        }]
        .into();
        data.signals.clips = vec![PreparedClip { id: 7, effect: 0 }].into();
        data.signals.effects_by_layer = vec![vec![0].into_boxed_slice()].into();
        data.signals.layers = vec![PreparedLayer { enabled: true }].into();
        data.signals.plan = SignalPlan {
            output_index: 1,
            target: 0,
            nodes: vec![
                PreparedSignalNode {
                    kind: PreparedSignalKind::Layer { layer_index: 0 },
                },
                PreparedSignalNode {
                    kind: PreparedSignalKind::Output {
                        inputs: vec![0].into(),
                    },
                },
            ]
            .into(),
            vm_workspace_count: 0,
            frame_nodes: vec![0, 1].into(),
            frame_slots: vec![0, 1].into(),
            frame_buffer_count: 2,
        };
        data.outputs[0].width = 12;
        data.patch.routes = vec![PreparedPixelRoute {
            pixels: 0..3,
            frame: 0,
            start_slot: 2,
            encoding: PixelEncoding::Rgb { order: [2, 1, 0] },
            lookup: None,
        }]
        .into();
        restore_fixture(data.signals, data.patch, data.outputs).unwrap()
    }

    fn empty_sequence() -> PreparedSequence {
        restore_fixture(
            PreparedSignalGraph {
                clips: Box::new([]),
                frame_rate: 60,
                frame_count: 60,
                duration: SampleDuration::from_ticks(1_000_000),
                fixtures: Box::new([]),
                fixture_pixel_offsets: Box::new([]),
                pixel_count: 0,
                effects: Box::new([]),
                programs: Box::new([]),
                targets: vec![PreparedTarget {
                    pixels: crate::targets::TargetInterner::default().prepare(vec![]),
                    spatial: Box::new([]),
                    sections: Default::default(),
                    sample_count: 0,
                }]
                .into(),
                positions: Box::new([]),
                effects_by_layer: Box::new([]),
                layers: Box::new([]),
                plan: SignalPlan {
                    output_index: 0,
                    target: 0,
                    nodes: vec![PreparedSignalNode {
                        kind: PreparedSignalKind::Output {
                            inputs: Box::new([]),
                        },
                    }]
                    .into(),
                    vm_workspace_count: 0,
                    frame_nodes: vec![0].into(),
                    frame_slots: vec![0].into(),
                    frame_buffer_count: 1,
                },
            },
            PreparedPatch {
                routes: Box::new([]),
                lookups: Box::new([]),
            },
            vec![PreparedOutput {
                controller_index: 3,
                port: 7,
                width: 3,
            }]
            .into(),
        )
        .unwrap()
    }

    fn queried_sequence(pixel: crate::dsl::bytecode::SignalPixel<i32>) -> PreparedSequence {
        use crate::signal::PreparedOperatorNode;
        let mut data = timed_sequence().archive_data();
        let mut programs = data.signals.programs.into_vec();
        let mut second = programs[0].clone();
        second.code[0] = Instruction::ColorConst {
            dst: Slot::scalar(0),
            value: Color {
                red: 5,
                green: 101,
                blue: 3,
            },
        };
        programs.push(second);
        // The query block loads the sample's time and pixel; the body samples.
        programs.push(BytecodeProgram {
            frame_caches: 1,
            ..program(
                vec![
                    Instruction::FloatConst {
                        dst: Slot::scalar(0),
                        bits: 0.3f32.to_bits(),
                    },
                    Instruction::IntConst {
                        dst: Slot::scalar(0),
                        value: pixel.index().copied().unwrap_or(0),
                    },
                    Instruction::Sample {
                        dst: Slot::row(0),
                        input: 0,
                        seconds: Slot::scalar(0),
                        pixel: pixel.map(|_| Slot::scalar(0)),
                        frame_cache: 0,
                    },
                ],
                2,
                Slot::row(0),
                Banks {
                    floats: 1,
                    ints: 1,
                    ..Banks::default()
                },
                Banks {
                    colors: 1,
                    ..Banks::default()
                },
            )
        });
        data.signals.programs = programs.into();
        let mut targets = data.signals.targets.into_vec();
        let mut interner = crate::targets::TargetInterner::default();
        let first_pixels = interner.prepare(targets[0].pixels.iter().take(2).collect());
        let last_pixels = interner.prepare(targets[0].pixels.iter().skip(2).collect());
        targets.extend([
            PreparedTarget {
                pixels: first_pixels,
                spatial: Box::new([]),
                sections: Default::default(),
                sample_count: 0,
            },
            PreparedTarget {
                pixels: last_pixels,
                spatial: Box::new([]),
                sections: Default::default(),
                sample_count: 0,
            },
        ]);
        data.signals.targets = targets.into();
        let mut effects = data.signals.effects.into_vec();
        effects[0].target = 1;
        let mut second = effects[0].clone();
        second.target = 2;
        second.program = 1;
        second.bound_params = crate::dsl::BoundParams::default();
        effects.push(second);
        data.signals.effects = effects.into();
        data.signals.effects_by_layer[0] = vec![0, 1].into();
        data.signals.plan = SignalPlan {
            output_index: 2,
            target: 0,
            nodes: vec![
                PreparedSignalNode {
                    kind: PreparedSignalKind::Layer { layer_index: 0 },
                },
                PreparedSignalNode {
                    kind: PreparedSignalKind::Operator {
                        operator: PreparedOperatorNode {
                            automation_slot: 0,
                            program: 2,
                            params: crate::dsl::BoundParams::default(),
                        },
                        inputs: vec![0].into(),
                        automation: Box::<[crate::signal::PreparedAutomation]>::default(),
                        vm_slot: 0,
                    },
                },
                PreparedSignalNode {
                    kind: PreparedSignalKind::Output {
                        inputs: vec![1].into(),
                    },
                },
            ]
            .into(),
            vm_workspace_count: 1,
            frame_nodes: vec![1].into(),
            frame_slots: vec![1, 0, 0].into(),
            frame_buffer_count: 1,
        };
        restore_fixture(data.signals, data.patch, data.outputs).unwrap()
    }

    #[test]
    fn local_and_global_temporal_queries_keep_their_domains_across_seeks() {
        use crate::dsl::bytecode::SignalPixel;
        let first = Color {
            red: 17,
            green: 29,
            blue: 43,
        };
        let second = Color {
            red: 5,
            green: 101,
            blue: 3,
        };
        for (pixel, expected) in [
            (SignalPixel::Current, [first, first, second]),
            (SignalPixel::Global(2), [second; 3]),
            (SignalPixel::Local(1), [first, first, Color::BLACK]),
            (SignalPixel::Global(-1), [Color::BLACK; 3]),
            (SignalPixel::Global(3), [Color::BLACK; 3]),
        ] {
            let mut playback = queried_sequence(pixel).into_playback();
            for time in [300_000, 900_000, 0, 500_000] {
                assert_eq!(
                    playback.evaluate(SampleTime::from_ticks(time)).colors(),
                    expected
                );
            }
        }
    }

    #[test]
    fn archive_roundtrip_preserves_output_metadata_and_empty_playback() {
        let sequence = empty_sequence();
        let bytes = crate::archive::encode_sequence(&sequence).unwrap();
        let decoded =
            crate::archive::decode_sequence(&bytes, crate::archive::LoadLimits::default()).unwrap();
        assert_eq!(decoded.outputs(), sequence.outputs());
        let mut playback = decoded.into_playback();
        playback.outputs[0].fill(255);
        let frame = playback.evaluate(SampleTime::from_ticks(500_000));
        assert_eq!(frame.outputs().next().unwrap().bytes, [0, 0, 0]);
    }

    #[test]
    fn owned_frames_preserve_seek_boundaries_fixture_order_and_output_storage() {
        let mut playback = timed_sequence().into_playback();
        let pointer = playback.outputs[0].as_ptr();
        for ticks in [
            300_000, 900_000, 300_000, 0, 1_000_000, 799_999, 200_000, 800_000,
        ] {
            let frame = playback.evaluate(SampleTime::from_ticks(ticks));
            let color = if (200_000..800_000).contains(&ticks) {
                Color {
                    red: 17,
                    green: 29,
                    blue: 43,
                }
            } else {
                Color::BLACK
            };
            assert_eq!(frame.colors(), &[color; 3]);
            let fixtures: alloc::vec::Vec<_> = frame.fixtures().collect();
            assert_eq!(
                fixtures[0],
                FixtureFrame {
                    fixture_id: 10,
                    pixels: &[color; 2]
                }
            );
            assert_eq!(
                fixtures[1],
                FixtureFrame {
                    fixture_id: 20,
                    pixels: &[color]
                }
            );
            let output = frame.outputs().next().unwrap();
            assert_eq!(output.bytes.as_ptr(), pointer);
            assert_eq!(output.output.controller_index, 3);
            assert_eq!(output.output.port, 7);
            assert_eq!(&output.bytes[..2], &[0, 0]);
            assert_eq!(output.bytes[11], 0);
            for pixel in output.bytes[2..11].as_chunks::<3>().0 {
                assert_eq!(pixel, &[color.blue, color.green, color.red]);
            }
        }
    }

    #[test]
    fn sparse_clip_owns_output_and_recomputes_after_backward_seek() {
        let sequence = timed_sequence();
        let mut sampler = sequence.clip(7).unwrap().sampler(2);
        let expected = Color {
            red: 17,
            green: 29,
            blue: 43,
        };
        let pointer = sampler.evaluate(SampleTime::from_ticks(300_000)).as_ptr();
        assert_eq!(
            sampler.evaluate(SampleTime::from_ticks(900_000)),
            &[Color::BLACK; 2]
        );
        let colors = sampler.evaluate(SampleTime::from_ticks(300_000));
        assert_eq!(colors, &[expected; 2]);
        assert_eq!(colors.as_ptr(), pointer);
    }
}
