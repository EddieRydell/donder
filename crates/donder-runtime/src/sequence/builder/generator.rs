//! Closed lowering of linked generator calls. Environment/source indices never
//! cross the builder boundary; every transfer comes from an admitted call edge.
use super::*;
use crate::bindings::{ParameterSource, PreparedParameterBinding, ResolvedParameterBinding};
use crate::dsl::generator::{
    EmissionExecution, GeneratorBinding, GeneratorContext, GeneratorInput, GeneratorInvocation,
    LinkedGenerator,
};
use crate::dsl::{
    AutomationPlan, CalculationProgram, ParameterLink, TargetItemValue, TargetValue, Type,
};
use alloc::collections::BTreeMap;

/// Accepted root values paired with their linked invocation and automation.
#[derive(Clone, Debug)]
pub struct GeneratorPlayback {
    invocation: GeneratorInvocation,
    params: BoundParams,
    types: Box<[Type]>,
    automation: AutomationPlan,
    empty_automation: AutomationPlan,
}

impl GeneratorPlayback {
    pub fn admit(
        generator: Shared<LinkedGenerator>,
        values: Vec<Value>,
        automation: Box<[PreparedAutomation]>,
        cache: &mut DslBindCache,
    ) -> Result<Self, RuntimeError> {
        if !generator.has_builder_target_provenance() {
            return Err(RuntimeError {
                message: "generator targets must originate from the builder context".into(),
            });
        }
        let invalid = || RuntimeError {
            message: "generator playback inputs do not match its linked declaration".into(),
        };
        let declarations = generator.params();
        if declarations.len() != values.len()
            || declarations
                .iter()
                .zip(&values)
                .any(|(declaration, value)| {
                    !declaration.ty.accepts_value(value) || has_target_records(value)
                })
            || automation.iter().any(|binding| {
                let index = usize::from(binding.param_index);
                declarations.get(index).is_none_or(|declaration| {
                    !declaration.supports_automation()
                        || !binding.mapping.accepts_type(&declaration.ty)
                })
            })
        {
            return Err(invalid());
        }
        let types: Box<[_]> = declarations
            .iter()
            .map(|declaration| declaration.ty.clone())
            .collect();
        let inputs = values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                if automation
                    .iter()
                    .any(|binding| usize::from(binding.param_index) == index)
                {
                    GeneratorInput::Live
                } else {
                    GeneratorInput::Fixed(value.clone())
                }
            })
            .collect();
        let invocation = generator.bind(inputs)?;
        let params = BoundParams::from_values(types.iter().zip(values), cache);
        let automation = AutomationPlan::admit(&params, &automation).ok_or_else(invalid)?;
        let empty_automation = AutomationPlan::admit(&params, &[]).ok_or_else(invalid)?;
        Ok(Self {
            invocation,
            params,
            types,
            automation,
            empty_automation,
        })
    }
}

fn has_target_records(value: &Value) -> bool {
    match value {
        Value::Target(target) => target.groups.iter().any(|group| !group.pixels.is_empty()),
        Value::TargetItems(target) => target.groups.iter().any(|group| !group.pixels.is_empty()),
        Value::TargetItem(target) => !target.pixels.is_empty(),
        Value::Array(values) => values.iter().any(has_target_records),
        _ => false,
    }
}

#[derive(Clone, Copy)]
pub struct GeneratedEffect<'id> {
    pub effect: EffectHandle<'id>,
    pub window: WindowHandle<'id>,
    pub target: TargetHandle<'id>,
}

struct Expansion<'a> {
    empty_automation: &'a AutomationPlan,
    cache: DslBindCache,
    // These are address layouts only, not per-frame buffers. They are discarded
    // with expansion; the frozen graph retains each calculation exactly once.
    layouts: Vec<BoundParams>,
}

impl<'id> SequenceBuilder<'id> {
    pub fn generator(
        &mut self,
        invocation: &GeneratorPlayback,
        window: WindowHandle<'id>,
        target: TargetHandle<'id>,
    ) -> Vec<GeneratedEffect<'id>> {
        let root = self.environments.len();
        self.environments.push(ExecutableEnvironment {
            start_time: window.start,
            duration: window.duration,
            params: invocation.params.clone(),
            types: invocation.types.clone(),
            bindings: Box::new([]),
            automation: invocation.automation.clone(),
            calculation: None,
            array_capacity: 0,
            array_width: 0,
        });
        let mut expansion = Expansion {
            empty_automation: &invocation.empty_automation,
            cache: DslBindCache::default(),
            layouts: self
                .environments
                .iter()
                .map(|environment| BoundParams::result_workspace(environment.output_types(), 0, 0))
                .collect(),
        };
        let mut effects = Vec::new();
        self.expand_generator(
            &invocation.invocation,
            root,
            window,
            target,
            &mut expansion,
            &mut effects,
        );
        effects
    }

    fn expand_generator(
        &mut self,
        invocation: &GeneratorInvocation,
        environment: usize,
        window: WindowHandle<'id>,
        target: TargetHandle<'id>,
        expansion: &mut Expansion<'_>,
        effects: &mut Vec<GeneratedEffect<'id>>,
    ) {
        let context = GeneratorContext {
            start_time: window.start,
            duration: window.duration,
            target: Shared::new(TargetValue {
                groups: vec![Shared::new(TargetItemValue {
                    pixels: Shared::clone(&self.targets[target.index].selection),
                })],
            }),
        };
        let (calculations, children) = invocation.specialize(&context).into_parts();
        let mut calculated = Vec::with_capacity(calculations.len());
        for calculation in calculations {
            let types = calculation.program.input_types().into();
            let index = self.linked_environment(
                types,
                &calculation.inputs,
                &calculation.links,
                environment,
                &calculated,
                window,
                Some(calculation.program),
                expansion,
            );
            calculated.push(index);
        }
        for emission in children {
            let child = emission.child();
            let Some(child_window) = self.emitted_window(child.start_time, child.duration) else {
                continue;
            };
            let child_target = self.emitted_target(target, Shared::clone(&child.target.pixels));
            let types = match emission.execution() {
                EmissionExecution::Sample(program) => program.input_types().into(),
                EmissionExecution::Generator(invocation) => invocation
                    .params()
                    .iter()
                    .map(|parameter| parameter.ty.clone())
                    .collect(),
            };
            // Every child gets its own declared layout. In particular a widened
            // float input cannot retain an alias to an ancestor's integer bank.
            let child_environment = self.linked_environment(
                types,
                emission.inputs(),
                emission.links(),
                environment,
                &calculated,
                child_window,
                None,
                expansion,
            );
            match emission.execution() {
                EmissionExecution::Generator(invocation) => self.expand_generator(
                    invocation,
                    child_environment,
                    child_window,
                    child_target,
                    expansion,
                    effects,
                ),
                EmissionExecution::Sample(program) => {
                    let program = match self
                        .sample_programs
                        .iter()
                        .position(|definition| Shared::ptr_eq(&definition.0, program))
                    {
                        Some(index) => index,
                        None => {
                            let index = self.sample_programs.len();
                            self.sample_programs
                                .push(SampleDefinition(Shared::clone(program)));
                            index
                        }
                    };
                    let environment = &self.environments[child_environment];
                    let implementation = if environment.bindings.is_empty() {
                        // Specialization already evaluated every fixed input.
                        // A constant leaf needs only its frozen parameter banks.
                        PreparedEffectImplementation::Dsl {
                            program,
                            bound_params: environment.params.clone(),
                        }
                    } else {
                        PreparedEffectImplementation::Bound {
                            environment: child_environment,
                            program,
                        }
                    };
                    let effect = EffectHandle::new(self.effects.len());
                    self.effects.push(PreparedEffect {
                        start_time: child_window.start,
                        duration: child_window.duration,
                        target: child_target.index,
                        implementation,
                        automation: None,
                    });
                    effects.push(GeneratedEffect {
                        effect,
                        window: child_window,
                        target: child_target,
                    });
                }
            }
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "private lowering carries one admitted call's schema, bindings and owning scope"
    )]
    fn linked_environment(
        &mut self,
        types: Box<[Type]>,
        inputs: &[GeneratorBinding],
        links: &[Option<ParameterLink>],
        parent: usize,
        calculations: &[usize],
        window: WindowHandle<'id>,
        calculation: Option<CalculationProgram>,
        expansion: &mut Expansion<'_>,
    ) -> usize {
        let values = inputs.iter().map(|input| match input {
            GeneratorBinding::Constant(value) => value.clone(),
            // Uninitialized typed storage, filled by the linked transfer before
            // execution. It is not a substitute value for a missing argument.
            GeneratorBinding::Parameter(_) | GeneratorBinding::Calculation { .. } => Value::Void,
        });
        let params = BoundParams::from_values(types.iter().zip(values), &mut expansion.cache);
        let sources = inputs
            .iter()
            .enumerate()
            .filter_map(|(destination, input)| {
                let source = match input {
                    GeneratorBinding::Constant(_) => return None,
                    GeneratorBinding::Parameter(parameter) => ParameterSource {
                        environment: parent,
                        parameter: *parameter,
                    },
                    GeneratorBinding::Calculation { index, output } => ParameterSource {
                        environment: calculations[*index],
                        parameter: *output,
                    },
                };
                Some((destination as u16, source))
            });
        // Private specialization emits exactly one proof per nonconstant input,
        // in declaration order. Callers never supply either of these streams.
        let mut array_capacity = 0usize;
        let mut array_width = 0usize;
        let bindings = sources
            .zip(links.iter().flatten())
            .map(|((parameter, mut source), link)| {
                // Pure forwarding has no clock of its own. Preserve every
                // conversion boundary, but resolve identical types directly
                // against their original bank layout before retaining storage.
                while let Some(forwarded) =
                    self.environments[source.environment].forwarded_source(source.parameter)
                {
                    let ancestor = &self.environments[source.environment];
                    let upstream = &self.environments[forwarded.environment];
                    if ancestor.output_types()[usize::from(source.parameter)]
                        != upstream.output_types()[usize::from(forwarded.parameter)]
                    {
                        break;
                    }
                    source = forwarded;
                }
                let ancestor = &self.environments[source.environment];
                array_capacity = array_capacity.saturating_add(ancestor.array_capacity);
                array_width = array_width.max(ancestor.array_width);
                let transfer = link.resolve(
                    &expansion.layouts[source.environment],
                    source.parameter,
                    &params,
                    parameter,
                );
                ResolvedParameterBinding::from_linked(
                    PreparedParameterBinding { parameter, source },
                    transfer,
                )
            })
            .collect();
        if let Some(calculation) = &calculation
            && calculation
                .output_types()
                .iter()
                .any(|ty| matches!(ty, Type::Array(_)))
        {
            array_capacity =
                array_capacity.saturating_add(calculation.bytecode().array_capacity as usize);
            array_width = array_width.max(calculation.bytecode().array_width as usize);
        }
        let output_types = calculation
            .as_ref()
            .map_or(types.as_ref(), CalculationProgram::output_types);
        expansion
            .layouts
            .push(BoundParams::result_workspace(output_types, 0, 0));
        let index = self.environments.len();
        self.environments.push(ExecutableEnvironment {
            start_time: window.start,
            duration: window.duration,
            params,
            types,
            bindings,
            automation: expansion.empty_automation.clone(),
            calculation,
            array_capacity,
            array_width,
        });
        index
    }

    fn emitted_window(
        &self,
        start: SampleTime,
        duration: SampleDuration,
    ) -> Option<WindowHandle<'id>> {
        if duration.as_ticks() == 0
            || start.checked_add_duration(duration)?.as_ticks() > self.timing.duration.get()
        {
            return None;
        }
        Some(WindowHandle {
            start,
            duration,
            brand: PhantomData,
        })
    }

    fn emitted_target(
        &mut self,
        parent: TargetHandle<'id>,
        selection: Shared<[PreparedPixel]>,
    ) -> TargetHandle<'id> {
        let parent = &self.targets[parent.index];
        let positions: BTreeMap<_, _> = parent
            .pixels
            .iter()
            .zip(&parent.spatial)
            .map(|(pixel, spatial)| {
                (
                    (pixel.fixture_index, pixel.fixture_pixel_index),
                    spatial.position,
                )
            })
            .collect();
        let mut pixels = selection.to_vec();
        pixels.sort_by_key(|pixel| (pixel.fixture_index, pixel.fixture_pixel_index));
        // Generated sections/pixels establish their own spatial scope. Logical
        // indices/counts remain inherited; the existing scope rule treats the
        // selection as whole-target only when every count equals its length.
        let whole_target = pixels.iter().all(|pixel| pixel.pixel_count == pixels.len());
        let mut bounds = BTreeMap::new();
        for pixel in &pixels {
            let position = positions[&(pixel.fixture_index, pixel.fixture_pixel_index)];
            let key = (!whole_target).then_some(pixel.fixture_index);
            let (min, max) = bounds.entry(key).or_insert((position, position));
            for axis in 0..2 {
                min[axis] = min[axis].min(position[axis]);
                max[axis] = max[axis].max(position[axis]);
            }
        }
        let spatial: Vec<_> = pixels
            .iter()
            .map(|pixel| {
                let (min, max) = bounds[&(!whole_target).then_some(pixel.fixture_index)];
                SpatialContext {
                    position: positions[&(pixel.fixture_index, pixel.fixture_pixel_index)],
                    min,
                    max,
                }
            })
            .collect();
        if let Some(index) = self.targets.iter().position(|target| {
            target.pixels == pixels && target.spatial == spatial && target.selection == selection
        }) {
            return TargetHandle::new(index);
        }
        let index = self.targets.len();
        self.targets.push(Target {
            pixels,
            spatial,
            selection,
        });
        TargetHandle::new(index)
    }
}

#[cfg(test)]
mod tests {
    mod retention;

    use super::*;
    use crate::dsl::bytecode::{
        BytecodeProgram, ColorSlot, ContextRead, FloatSlot, Instruction, NumberSlot, PoolSpan,
        SlotLayout, TargetItemSlot, ValueSlot,
    };
    use crate::dsl::generator::{
        FixedCalculation, GeneratedEffectSlot, GeneratorProgram, GeneratorTarget, Statement,
    };

    fn raw_program(instructions: Vec<Instruction>, layout: SlotLayout) -> BytecodeProgram {
        BytecodeProgram {
            instructions: instructions.into(),
            array_constants: Box::new([]),
            enums: Box::new([]),
            enum_types: Box::new([]),
            curves: Box::new([]),
            targets: Box::new([]),
            target_lists: Box::new([]),
            target_items: Box::new([]),
            gradients: Box::new([]),
            value_operands: Box::new([]),
            array_types: Box::new([]),
            layout,
            uses_pixel_context: false,
            pixel_entry: 0,
            array_capacity: 0,
            array_width: 0,
            loop_count: 0,
        }
    }

    fn fixed_seconds(value: f32) -> Box<FixedCalculation<f32>> {
        let mut raw = raw_program(
            vec![
                Instruction::LoadFloatConst {
                    dst: FloatSlot(0),
                    bits: value.to_bits(),
                },
                Instruction::ReturnValues(PoolSpan { start: 0, len: 1 }),
            ],
            SlotLayout {
                floats: 1,
                ..SlotLayout::default()
            },
        );
        raw.value_operands = vec![ValueSlot::Float(FloatSlot(0))].into();
        Box::new(FixedCalculation {
            program: CalculationProgram::new(raw, Box::new([]), vec![Type::Float].into())
                .unwrap()
                .into_output()
                .unwrap(),
            inputs: Box::new([]),
        })
    }

    #[test]
    fn playback_admission_rejects_embedded_targets_that_bypass_builder_coordinates() {
        for logical_index in [0, 3] {
            let pixel = PreparedPixel {
                fixture_index: 0,
                fixture_pixel_index: 0,
                pixel_index: logical_index,
                pixel_count: 1,
                pixel_fraction: 0.0,
            };
            let mut raw = raw_program(
                vec![
                    Instruction::LoadTargetItemConst {
                        dst: TargetItemSlot(0),
                        constant: 0,
                    },
                    Instruction::ReturnValues(PoolSpan { start: 0, len: 1 }),
                ],
                SlotLayout {
                    target_items: 1,
                    ..SlotLayout::default()
                },
            );
            // Index zero formerly produced an archive rejected for duplicate
            // physical addresses; index three also panicked in the sample cache.
            raw.target_items = vec![Shared::new(TargetItemValue {
                pixels: vec![pixel, pixel].into(),
            })]
            .into();
            raw.value_operands = vec![ValueSlot::TargetItem(TargetItemSlot(0))].into();
            let target = FixedCalculation {
                program: CalculationProgram::new(raw, Box::new([]), vec![Type::TargetItem].into())
                    .unwrap()
                    .into_output()
                    .unwrap(),
                inputs: Box::new([]),
            };
            let generator = GeneratorProgram::admit(
                vec![],
                vec![Statement::Emit {
                    slot: GeneratedEffectSlot(0),
                    start: fixed_seconds(0.0),
                    duration: fixed_seconds(1.0),
                    target: Box::new(target),
                    params: vec![],
                }],
                vec![Type::Target, Type::Float].into(),
                vec![Box::new([]) as Box<[_]>].into(),
            )
            .unwrap();
            let mut sample = raw_program(
                vec![
                    Instruction::ContextRead {
                        dst: NumberSlot::Float(FloatSlot(0)),
                        read: ContextRead::PixelFraction,
                    },
                    Instruction::Rgb {
                        dst: ColorSlot(0),
                        red: FloatSlot(0),
                        green: FloatSlot(0),
                        blue: FloatSlot(0),
                    },
                    Instruction::ReturnColor(ColorSlot(0)),
                ],
                SlotLayout {
                    floats: 1,
                    colors: 1,
                    ..SlotLayout::default()
                },
            );
            sample.uses_pixel_context = true;
            let sample = SampleProgram::admit(sample, Box::new([])).unwrap();
            let linked = LinkedGenerator::link(
                Shared::new(generator),
                vec![GeneratorTarget::Sample {
                    program: Shared::new(sample),
                    params: Box::new([]),
                }]
                .into(),
            )
            .unwrap();
            assert!(
                GeneratorPlayback::admit(
                    linked,
                    vec![],
                    Box::new([]),
                    &mut DslBindCache::default(),
                )
                .is_err()
            );
        }
    }

    #[test]
    fn emitted_sections_recompute_bounds_without_resetting_logical_coordinates() {
        let timing = SequenceTiming::admit(
            NonZeroU32::new(60).unwrap(),
            NonZeroU32::new(60).unwrap(),
            NonZeroU32::new(1_000_000).unwrap(),
            Box::new([]),
        )
        .unwrap();
        let mut builder = SequenceBuilder::new(timing);
        let a = builder.fixture(
            10,
            FixtureGeometry::admit(vec![[0.0, 0.0], [2.0, 1.0], [4.0, 2.0]].into()).unwrap(),
        );
        let b = builder.fixture(
            20,
            FixtureGeometry::admit(vec![[20.0, 3.0], [22.0, 4.0], [24.0, 5.0]].into()).unwrap(),
        );
        let root = builder.target([b, a], TargetScope::WholeTarget);
        let original = builder.targets[root.index].pixels.clone();
        let section = builder.emitted_target(
            root,
            vec![original[4], original[3], original[2], original[1]].into(),
        );
        let child = &builder.targets[section.index];
        assert_eq!(child.pixels, original[1..5]);
        assert_eq!(
            child.selection.as_ref(),
            [original[4], original[3], original[2], original[1]]
        );
        assert_eq!(child.spatial[0].min, [2.0, 1.0]);
        assert_eq!(child.spatial[0].max, [4.0, 2.0]);
        assert_eq!(child.spatial[2].min, [20.0, 3.0]);
        assert_eq!(child.spatial[2].max, [22.0, 4.0]);
        assert_eq!(child.pixels[0].pixel_index, original[1].pixel_index);
        assert_eq!(child.pixels[0].pixel_count, 6);
        let pixel = builder.emitted_target(section, vec![original[4]].into());
        assert_eq!(
            builder.targets[pixel.index].spatial[0],
            SpatialContext {
                position: [22.0, 4.0],
                min: [22.0, 4.0],
                max: [22.0, 4.0],
            }
        );
        // Output slicing is not a new emission: it keeps that emitted scope.
        let routed = builder.target_slice(section, 0..1);
        assert_eq!(builder.targets[routed.index].spatial[0].max, [4.0, 2.0]);
        let whole = builder.emitted_target(root, original.into());
        assert!(
            builder.targets[whole.index]
                .spatial
                .iter()
                .all(|context| context.min == [0.0, 0.0] && context.max == [24.0, 5.0])
        );
    }
}
