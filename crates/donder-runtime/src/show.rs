//! A show image borrowed for playback: the records preparation wrote, read in
//! place. Admission checks every cross-reference and every program once, so
//! playback indexes records without checking them again.
use donder_runtime_types::bytecode::{
    Banks, ParameterKind, ProgramContext, ProgramShape, Resource, Slot, well_formed,
};
use donder_runtime_types::image::{self, NONE, Range, Sections};
use donder_runtime_types::{InstructionWord, SampleDuration, SampleTime};

/// Current image format accepted by this runtime.
pub const FORMAT_VERSION: u32 = 65;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadError {
    Header,
    Version,
    Checksum,
    Archive,
    Limit,
}

impl From<image::FrameError> for LoadError {
    fn from(error: image::FrameError) -> Self {
        match error {
            image::FrameError::Header => Self::Header,
            image::FrameError::Version => Self::Version,
            image::FrameError::Checksum => Self::Checksum,
            image::FrameError::Limit => Self::Limit,
            image::FrameError::Sections => Self::Archive,
        }
    }
}

/// Admission limits for images from a trusted Donder compiler.
#[derive(Clone, Copy, Debug)]
pub struct LoadLimits {
    /// Whole image, header included.
    pub image_bytes: usize,
    pub pixels: usize,
    pub graph_nodes: usize,
    /// The playback buffer [`Show::workspace_bytes`] asks for.
    pub workspace_bytes: usize,
}

impl LoadLimits {
    /// No limit but the format's own, for images this process built.
    pub const UNLIMITED: Self = Self {
        image_bytes: usize::MAX,
        pixels: usize::MAX,
        graph_nodes: usize::MAX,
        workspace_bytes: usize::MAX,
    };
}

impl Default for LoadLimits {
    fn default() -> Self {
        Self {
            image_bytes: 256 * 1024,
            pixels: 4096,
            graph_nodes: 256,
            workspace_bytes: 128 * 1024,
        }
    }
}

/// An image's total length from its first [`image::header_bytes`] bytes, so
/// a transport can bound an upload before receiving it.
pub fn image_len(header: &[u8], limits: LoadLimits) -> Result<usize, LoadError> {
    let len = image::image_len(header, FORMAT_VERSION)?;
    if len > limits.image_bytes {
        return Err(LoadError::Limit);
    }
    Ok(len)
}

pub use donder_runtime_types::image::header_bytes;

/// An admitted image.
#[derive(Clone, Copy, Debug)]
pub struct Show<'a> {
    pub(crate) s: Sections<'a>,
}

impl<'a> Show<'a> {
    /// Check `bytes` and borrow them. `bytes` must be 4-byte aligned.
    pub fn admit(bytes: &'a [u8], limits: LoadLimits) -> Result<Self, LoadError> {
        let s = image::borrow(bytes, FORMAT_VERSION, limits.image_bytes)?;
        let show = Self { s };
        if show.s.meta.len() != 1 || !show.consistent() {
            return Err(LoadError::Archive);
        }
        if show.pixel_count() > limits.pixels || show.s.nodes.len() > limits.graph_nodes {
            return Err(LoadError::Limit);
        }
        let layout = crate::workspace::Layout::of(show);
        if layout.frame_caches() > crate::workspace::MAX_FRAMES {
            return Err(LoadError::Limit);
        }
        if layout.size() > limits.workspace_bytes {
            return Err(LoadError::Limit);
        }
        Ok(show)
    }

    /// Borrow an image this process admitted before.
    pub fn trusted(bytes: &'a [u8]) -> Self {
        let Some(s) = image::borrow_trusted(bytes) else {
            unreachable!("only admitted images are re-borrowed")
        };
        Self { s }
    }

    pub(crate) fn meta(&self) -> &'a image::Meta {
        &self.s.meta[0]
    }

    pub fn frame_rate(&self) -> u32 {
        self.meta().frame_rate
    }

    pub fn frame_count(&self) -> u32 {
        self.meta().frame_count
    }

    pub fn duration(&self) -> SampleDuration {
        SampleDuration::from_ticks(self.meta().duration)
    }

    pub fn pixel_count(&self) -> usize {
        self.meta().pixel_count as usize
    }

    pub fn fixtures(&self) -> &'a [image::Fixture] {
        self.s.fixtures
    }

    pub fn outputs(&self) -> &'a [image::Output] {
        self.s.outputs
    }

    pub fn effect_count(&self) -> usize {
        self.s.effects.len()
    }

    /// The playback buffer this show needs.
    pub fn layout(&self) -> crate::workspace::Layout {
        crate::workspace::Layout::of(*self)
    }

    /// Effects active at `time`, for diagnostics.
    pub fn active_effect_count(&self, time: SampleTime) -> usize {
        let ticks = time.as_ticks();
        self.s
            .effects
            .iter()
            .filter(|effect| ticks >= effect.start && ticks - effect.start < effect.duration)
            .count()
    }

    pub(crate) fn program(&self, index: u32) -> ProgramView<'a> {
        let record = &self.s.programs[index as usize];
        ProgramView {
            record,
            code: record.code.get(self.s.code),
            params: record.params.get(self.s.params),
            operands: record.operands.get(self.s.operands),
            curves: record.curves.get(self.s.program_resources),
            gradients: record.gradients.get(self.s.program_resources),
            marks: record.marks.get(self.s.program_resources),
            arrays: record.arrays.get(self.s.program_resources),
        }
    }

    /// Whether every record refers only to records that exist, of the kinds
    /// it expects, and every program is well formed for its use.
    fn consistent(&self) -> bool {
        let s = &self.s;
        let meta = &s.meta[0];
        let within = |range: Range, count: usize| range.within(count);
        let index = |value: u32, count: usize| (value as usize) < count;
        let optional = |value: u32, count: usize| value == NONE || index(value, count);
        let fixtures = s.fixtures.iter().all(|fixture| {
            within(fixture.positions, s.positions.len())
                && (fixture.positions.len == 0 || fixture.positions.len == fixture.pixel_count)
                && (fixture.offset as usize)
                    .checked_add(fixture.pixel_count as usize)
                    .is_some_and(|end| end <= meta.pixel_count as usize)
        });
        let targets = s
            .targets
            .iter()
            .all(|target| self.target_consistent(target));
        let programs = s.programs.iter().enumerate().all(|(index, program)| {
            within(program.code, s.code.len())
                && within(program.params, s.params.len())
                && within(program.operands, s.operands.len())
                && [
                    program.curves,
                    program.gradients,
                    program.marks,
                    program.arrays,
                ]
                .iter()
                .zip([
                    image::RESOURCE_CURVE,
                    image::RESOURCE_GRADIENT,
                    image::RESOURCE_MARKS,
                    image::RESOURCE_ARRAY,
                ])
                .all(|(&range, kind)| {
                    within(range, s.program_resources.len())
                        && range.get(s.program_resources).iter().all(|resource| {
                            resource.kind == kind && self.resource_exists(*resource)
                        })
                })
                && self
                    .program(index as u32)
                    .params
                    .iter()
                    .all(|param| ParameterKind::from_wire(param.kind).is_some())
        });
        let well_formed_for = |program: u32, context: ProgramContext| {
            index(program, s.programs.len()) && well_formed(&self.program(program), context)
        };
        let params = s.param_sets.iter().all(|set| {
            within(set.words, s.words.len())
                && within(set.resources, s.resources.len())
                && set
                    .resources
                    .get(s.resources)
                    .iter()
                    .all(|resource| self.resource_exists(*resource))
        });
        let effects = s.effects.iter().all(|effect| {
            index(effect.target, s.targets.len())
                && index(effect.params, s.param_sets.len())
                && optional(effect.automation, s.automation.len())
                && well_formed_for(effect.program, ProgramContext::Effect)
                && s.programs[effect.program as usize].flags & image::PROGRAM_OPERATOR == 0
                && self.params_fit(effect.program, effect.params)
                && (effect.automation == NONE
                    || self.automation_fits(effect.program, effect.params, effect.automation))
        });
        let layers = s.layers.iter().all(|layer| {
            within(layer.effects, s.layer_order.len())
                && layer
                    .effects
                    .get(s.layer_order)
                    .iter()
                    .all(|&effect| index(effect, s.effects.len()))
        });
        // Ends are each layer's running latest end, in its start order.
        let ends = || {
            s.layer_ends.len() == s.layer_order.len()
                && s.layers.iter().all(|layer| {
                    let mut latest = 0u32;
                    let mut start = 0u32;
                    layer
                        .effects
                        .get(s.layer_order)
                        .iter()
                        .zip(layer.effects.get(s.layer_ends))
                        .all(|(&effect, &end)| {
                            let effect = &s.effects[effect as usize];
                            let sorted = effect.start >= start;
                            start = effect.start;
                            latest = latest.max(effect.start.saturating_add(effect.duration));
                            sorted && end == latest
                        })
                })
        };
        // Effects with automation or live marks have states, numbered in order.
        let mut next = 0;
        let states = s.effects.iter().enumerate().all(|(index, effect)| {
            let read = s
                .readers
                .iter()
                .any(|reader| reader.effect as usize == index);
            if effect.automation != NONE || read {
                next += 1;
                effect.state == next - 1
            } else {
                effect.state == NONE
            }
        });
        let nodes = s.nodes.iter().enumerate().all(|(position, node)| {
            within(node.inputs, s.node_inputs.len())
                // Inputs come before their consumers, so the graph has no cycle.
                && node
                    .inputs
                    .get(s.node_inputs)
                    .iter()
                    .all(|&input| (input as usize) < position)
                && match node.kind {
                    image::NODE_LAYER => index(node.layer, s.layers.len()),
                    image::NODE_MIX => true,
                    image::NODE_OPERATOR => {
                        well_formed_for(
                            node.program,
                            ProgramContext::Operator {
                                inputs: node.inputs.len as usize,
                            },
                        ) && s.programs[node.program as usize].flags & image::PROGRAM_OPERATOR != 0
                            && s.programs[node.program as usize].inputs == node.inputs.len
                            && index(node.params, s.param_sets.len())
                            && self.params_fit(node.program, node.params)
                            && index(node.vm_slot, meta.vm_slots as usize)
                            && (node.automation == NONE
                                || index(node.automation, s.automation.len())
                                    && self.automation_fits(
                                        node.program,
                                        node.params,
                                        node.automation,
                                    ))
                    }
                    _ => false,
                }
        });
        let resources = s.curves.iter().all(|curve| {
            within(curve.points, s.curve_points.len())
                && within(curve.areas, s.curve_areas.len())
                && curve.areas.len == curve.points.len
                && within(curve.crossings, s.crossings.len())
                && curve.crossings_kind <= image::CROSSINGS_MIXED
        }) && s
            .gradients
            .iter()
            .all(|gradient| within(gradient.stops, s.gradient_stops.len()))
            && s.marks.iter().all(|marks| {
                within(marks.track, s.tracks.len())
                    && marks.first <= marks.end
                    && marks.end <= marks.track.len
            })
            && s.arrays.iter().all(|array| {
                within(array.items, s.values.len())
                    && array
                        .items
                        .get(s.values)
                        .iter()
                        .all(|value| match value.kind {
                            image::VALUE_MARKS => index(value.word, s.marks.len()),
                            image::VALUE_CURVE => index(value.word, s.curves.len()),
                            image::VALUE_GRADIENT => index(value.word, s.gradients.len()),
                            image::VALUE_ARRAY => index(value.word, s.arrays.len()),
                            kind => kind <= image::VALUE_ENUM,
                        })
            });
        let automation = s.automation.iter().all(|plan| {
            within(plan.bindings, s.bindings.len())
                && within(plan.windows, s.windows.len())
                && plan.bindings.get(s.bindings).iter().all(|binding| {
                    index(binding.curve, s.curves.len())
                        && binding.kind <= image::BINDING_CURVE
                        && binding.duration != 0
                        && within(binding.options, s.enum_options.len())
                        && (binding.kind != image::BINDING_ENUM || binding.options.len != 0)
                        && (binding.kind != image::BINDING_CURVE
                            || index(binding.destination, plan.windows.len as usize))
                })
        });
        let patch = s.routes.iter().all(|route| {
            let channels = if route.encoding == image::ENCODING_RGBW {
                4
            } else {
                3
            };
            route.encoding <= image::ENCODING_RGBW
                && route.first <= route.end
                && route.end <= meta.pixel_count
                && optional(route.lookup, s.lookups.len())
                && s.outputs.get(route.output as usize).is_some_and(|output| {
                    ((route.end - route.first) as usize)
                        .checked_mul(channels)
                        .and_then(|width| width.checked_add(route.start_slot as usize))
                        .is_some_and(|end| end <= output.width as usize)
                })
                && route.order[..channels]
                    .iter()
                    .all(|&channel| usize::from(channel) < channels)
        });
        let rig = s
            .voices
            .iter()
            .all(|voice| index(voice.effect, s.effects.len()))
            && s.collections.iter().all(|collection| {
                within(collection.readers, s.readers.len())
                    && collection.readers.get(s.readers).iter().all(|reader| {
                        index(reader.effect, s.effects.len()) && {
                            let effect = &s.effects[reader.effect as usize];
                            let set = &s.param_sets[effect.params as usize];
                            set.resources
                                .get(s.resources)
                                .get(reader.bank as usize)
                                .is_some_and(|resource| resource.kind == image::RESOURCE_MARKS)
                        }
                    })
            })
            && s.controls.iter().all(|control| {
                within(control.targets, s.control_targets.len())
                    && control.targets.get(s.control_targets).iter().all(|target| {
                        index(target.effect, s.effects.len()) && {
                            let effect = &s.effects[target.effect as usize];
                            effect.automation != NONE
                                && target.binding
                                    < s.automation[effect.automation as usize].bindings.len
                        }
                    })
            });
        let plan = index(meta.plan_target, s.targets.len())
            && crate::targets::TargetView::of(*self, &s.targets[meta.plan_target as usize]).len()
                == meta.pixel_count as usize
            && optional(meta.output, s.nodes.len())
            && meta.vm_slots as usize <= crate::workspace::MAX_VM_SLOTS;
        let mut offset = 0u32;
        let outputs = s.outputs.iter().all(|output| {
            let fits = output.offset == offset;
            offset = offset.saturating_add(output.width);
            fits
        });
        fixtures
            && targets
            && programs
            && params
            && effects
            && layers
            && ends()
            && states
            && nodes
            && resources
            && automation
            && patch
            && rig
            && plan
            && outputs
            && s.clips
                .iter()
                .all(|clip| index(clip.effect, s.effects.len()))
            && s.clips.windows(2).all(|pair| pair[0].id < pair[1].id)
    }

    fn resource_exists(&self, resource: image::ResourceRef) -> bool {
        let s = &self.s;
        (resource.index as usize)
            < match resource.kind {
                image::RESOURCE_CURVE => s.curves.len(),
                image::RESOURCE_GRADIENT => s.gradients.len(),
                image::RESOURCE_MARKS => s.marks.len(),
                image::RESOURCE_ARRAY => s.arrays.len(),
                _ => 0,
            }
    }

    /// Whether parameter set `set` has the layout of `program`'s parameters:
    /// a word per word parameter and a resource of each resource
    /// parameter's kind.
    fn params_fit(&self, program: u32, set: u32) -> bool {
        let s = &self.s;
        let set = &s.param_sets[set as usize];
        let resources = set.resources.get(s.resources);
        let mut words = 0;
        let mut banks = 0;
        for param in self.program(program).params {
            let Some(kind) = ParameterKind::from_wire(param.kind) else {
                return false;
            };
            match kind.storage() {
                None => {}
                Some(donder_runtime_types::bytecode::ParamStorage::Word) => words += 1,
                Some(donder_runtime_types::bytecode::ParamStorage::Resource) => {
                    let expected = match kind {
                        ParameterKind::Curve => image::RESOURCE_CURVE,
                        ParameterKind::Gradient => image::RESOURCE_GRADIENT,
                        ParameterKind::Marks => image::RESOURCE_MARKS,
                        _ => image::RESOURCE_ARRAY,
                    };
                    if resources.get(banks).map(|resource| resource.kind) != Some(expected) {
                        return false;
                    }
                    banks += 1;
                }
            }
        }
        words == set.words.len as usize && banks == resources.len()
    }

    /// Whether `plan` writes only words and curve resources of `program`'s
    /// parameters bound in `set`.
    fn automation_fits(&self, program: u32, set: u32, plan: u32) -> bool {
        let s = &self.s;
        let plan = &s.automation[plan as usize];
        let set = &s.param_sets[set as usize];
        let program = self.program(program);
        let resources = set.resources.get(s.resources);
        let windows_fit = plan.windows.get(s.windows).iter().all(|window| {
            resources
                .get(window.resource as usize)
                .is_some_and(|resource| resource.kind == image::RESOURCE_CURVE)
        });
        windows_fit
            && plan.bindings.get(s.bindings).iter().all(|binding| {
                program.param_kind(binding.parameter as usize).is_some()
                    && (binding.kind == image::BINDING_CURVE || binding.destination < set.words.len)
            })
    }

    fn target_consistent(&self, target: &image::Target) -> bool {
        let s = &self.s;
        let fixture_cells = |fixture: u32, cell: u32, len: u32| {
            s.fixtures.get(fixture as usize).is_some_and(|record| {
                cell.checked_add(len)
                    .is_some_and(|end| end <= record.pixel_count)
            })
        };
        let pixels = match target.kind {
            image::PIXELS_RUNS => {
                target.pixels.within(s.pixel_runs.len()) && {
                    let mut start = 0;
                    target.pixels.get(s.pixel_runs).iter().all(|run| {
                        let fits = run.end > start
                            && fixture_cells(run.fixture, run.first_cell, run.end - start);
                        start = run.end;
                        fits
                    })
                }
            }
            image::PIXELS_INDEXED => {
                target.pixels.within(s.pixels.len())
                    && target
                        .pixels
                        .get(s.pixels)
                        .iter()
                        .all(|pixel| fixture_cells(pixel.fixture, pixel.cell, 1))
            }
            _ => false,
        };
        let len = crate::targets::TargetView::of(*self, target).len() as u32;
        let ascending = |ends: &mut dyn Iterator<Item = u32>| {
            let mut previous = 0;
            for end in ends {
                if end <= previous {
                    return false;
                }
                previous = end;
            }
            previous <= len
        };
        let spatial = target.spatial.within(s.spatial_runs.len())
            && ascending(&mut target.spatial.get(s.spatial_runs).iter().map(|run| run.end));
        let runs = target.section_runs.within(s.section_runs.len());
        let run_count = target.section_runs.len;
        let section_pixels = match target.section_kind {
            image::PIXELS_RUNS => {
                target.section_pixels.within(s.section_spans.len())
                    && ascending(
                        &mut target
                            .section_pixels
                            .get(s.section_spans)
                            .iter()
                            .map(|span| span.end),
                    )
                    && target
                        .section_pixels
                        .get(s.section_spans)
                        .iter()
                        .all(|span| span.first.run < run_count)
            }
            image::PIXELS_INDEXED => {
                target.section_pixels.within(s.section_pixels.len())
                    && target
                        .section_pixels
                        .get(s.section_pixels)
                        .iter()
                        .all(|pixel| pixel.run < run_count)
            }
            _ => false,
        };
        pixels && spatial && runs && section_pixels
    }
}

/// A program of an image.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ProgramView<'a> {
    pub(crate) record: &'a image::Program,
    pub(crate) code: &'a [InstructionWord],
    pub(crate) params: &'a [image::Param],
    pub(crate) operands: &'a [Slot],
    pub(crate) curves: &'a [image::ResourceRef],
    pub(crate) gradients: &'a [image::ResourceRef],
    pub(crate) marks: &'a [image::ResourceRef],
    pub(crate) arrays: &'a [image::ResourceRef],
}

fn banks(counts: image::BankCounts) -> Banks {
    let [floats, ints, bools, colors, resources, _] = counts.counts;
    Banks {
        floats,
        ints,
        bools,
        colors,
        resources,
    }
}

impl ProgramView<'_> {
    pub(crate) fn flag(&self, flag: u32) -> bool {
        self.record.flags & flag != 0
    }

    pub(crate) fn frame_cache_count(&self) -> usize {
        self.record.frame_caches as usize
    }

    pub(crate) fn slots(&self) -> (Banks, Banks, u16) {
        (
            banks(self.record.scalars),
            banks(self.record.rows),
            self.record.depth as u16,
        )
    }
}

impl ProgramShape for ProgramView<'_> {
    type Word = InstructionWord;
    fn code(&self) -> &[InstructionWord] {
        self.code
    }
    fn query_end(&self) -> u16 {
        u16::try_from(self.record.query_end).unwrap_or(u16::MAX)
    }
    fn target_end(&self) -> u16 {
        u16::try_from(self.record.target_end).unwrap_or(u16::MAX)
    }
    fn result(&self) -> Slot {
        Slot::from_bits(self.record.result as u16)
    }
    fn scalars(&self) -> Banks {
        banks(self.record.scalars)
    }
    fn rows(&self) -> Banks {
        banks(self.record.rows)
    }
    fn depth(&self) -> u16 {
        u16::try_from(self.record.depth).unwrap_or(u16::MAX)
    }
    fn param_kind(&self, index: usize) -> Option<ParameterKind> {
        ParameterKind::from_wire(self.params.get(index)?.kind)
    }
    fn operands(&self) -> &[Slot] {
        self.operands
    }
    fn constants(&self, kind: Resource) -> usize {
        match kind {
            Resource::Curve => self.curves.len(),
            Resource::Gradient => self.gradients.len(),
            Resource::Marks => self.marks.len(),
            Resource::Array => self.arrays.len(),
        }
    }
    fn frame_caches(&self) -> u16 {
        u16::try_from(self.record.frame_caches).unwrap_or(u16::MAX)
    }
}
