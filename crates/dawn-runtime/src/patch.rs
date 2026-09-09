use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::element::RenderedElementState;
use crate::fixture::{DimmingCurve, FixtureState, apply_dimming_curve, quantize8, quantize16};
use crate::fixture::{FixtureEncodingError, FixtureProgram};
use crate::values::Color;

#[derive(Clone, Copy, Debug, Eq, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum PatchValueLayout {
    Color(u32),
    Scalar(u32),
    Indexed(u32),
    Fixture { width: u32, functions: u32 },
    Components(u32),
    Slots(u32),
}

#[derive(Clone, Debug, PartialEq)]
pub enum PatchValue {
    Colors(Vec<Color>),
    Scalars(Vec<f32>),
    Indexed(Vec<u32>),
    FixtureStates(Vec<FixtureState>),
    Components(Vec<f32>),
    Slots(Vec<u8>),
}

impl PatchValue {
    pub fn new(layout: PatchValueLayout) -> Self {
        match layout {
            PatchValueLayout::Color(width) => Self::Colors(Vec::with_capacity(width as usize)),
            PatchValueLayout::Scalar(width) => Self::Scalars(Vec::with_capacity(width as usize)),
            PatchValueLayout::Indexed(width) => Self::Indexed(Vec::with_capacity(width as usize)),
            PatchValueLayout::Components(width) => {
                Self::Components(Vec::with_capacity(width as usize))
            }
            PatchValueLayout::Slots(width) => Self::Slots(Vec::with_capacity(width as usize)),
            PatchValueLayout::Fixture { width, functions } => Self::FixtureStates(
                (0..width)
                    .map(|_| FixtureState {
                        functions: Vec::with_capacity(functions as usize),
                    })
                    .collect(),
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum ByteOrder {
    CoarseFine,
    FineCoarse,
}

#[derive(Clone, Debug, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum PreparedFilter {
    ScalarToComponents {
        width: u32,
    },
    PackRgb {
        cell_count: u32,
        order: [u8; 3],
        lookup: Option<Box<[u8; 256]>>,
    },
    ColorBreakdown {
        capability: ColorEncoding,
        cell_count: u32,
    },
    DimmingCurve {
        curve: DimmingCurve,
        width: u32,
    },
    ScaleInvert {
        scale: f32,
        invert: bool,
        width: u32,
    },
    FanOut {
        width: u32,
        outputs: u16,
    },
    ComponentReorder {
        components_per_cell: u16,
        order: Box<[u16]>,
        cell_count: u32,
    },
    IndexedValueMapping {
        entries: Box<[(u32, f32)]>,
        width: u32,
    },
    Quantize8 {
        width: u32,
    },
    Quantize16 {
        width: u32,
        byte_order: ByteOrder,
    },
}

#[derive(Clone, Debug, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum ColorEncoding {
    Rgb,
    Rgbw,
    Discrete {
        colors: Box<[Color]>,
        levels: Box<[f32]>,
        components: u32,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum FilterError {
    TypeMismatch,
    WidthMismatch { expected: usize, actual: usize },
    UnsupportedDiscreteColor(Color),
    MissingIndexedMapping(u32),
}

impl PreparedFilter {
    pub fn evaluate(
        &self,
        input: &PatchValue,
        outputs: &mut [PatchValue],
    ) -> Result<(), FilterError> {
        match (self, input) {
            (Self::ScalarToComponents { width }, PatchValue::Scalars(values)) => {
                check_width(*width as usize, values.len())?;
                let [PatchValue::Components(output)] = outputs else {
                    return Err(FilterError::TypeMismatch);
                };
                output.clear();
                output.extend_from_slice(values);
            }
            (
                Self::PackRgb {
                    cell_count,
                    order,
                    lookup,
                },
                PatchValue::Colors(colors),
            ) => {
                check_width(*cell_count as usize, colors.len())?;
                let [PatchValue::Slots(output)] = outputs else {
                    return Err(FilterError::TypeMismatch);
                };
                output.clear();
                if let Some(lookup) = lookup {
                    for color in colors {
                        let channels = [color.red, color.green, color.blue];
                        output.extend(
                            order.map(|index| lookup[usize::from(channels[usize::from(index)])]),
                        );
                    }
                } else {
                    for color in colors {
                        let channels = [color.red, color.green, color.blue];
                        output.extend(order.map(|index| channels[usize::from(index)]));
                    }
                }
            }
            (
                Self::ColorBreakdown {
                    capability,
                    cell_count,
                },
                PatchValue::Colors(colors),
            ) => {
                check_width(*cell_count as usize, colors.len())?;
                let [PatchValue::Components(components)] = outputs else {
                    return Err(FilterError::TypeMismatch);
                };
                components.clear();
                for color in colors {
                    let rgb = [
                        f32::from(color.red) / 255.0,
                        f32::from(color.green) / 255.0,
                        f32::from(color.blue) / 255.0,
                    ];
                    match capability {
                        ColorEncoding::Rgb => components.extend(rgb),
                        ColorEncoding::Rgbw => {
                            let white = rgb[0].min(rgb[1]).min(rgb[2]);
                            components.extend([
                                rgb[0] - white,
                                rgb[1] - white,
                                rgb[2] - white,
                                white,
                            ]);
                        }
                        ColorEncoding::Discrete {
                            colors,
                            levels,
                            components: width,
                        } => {
                            let index = colors
                                .iter()
                                .position(|candidate| candidate == color)
                                .ok_or(FilterError::UnsupportedDiscreteColor(*color))?;
                            let start = index * *width as usize;
                            components.extend_from_slice(&levels[start..start + *width as usize]);
                        }
                    }
                }
            }
            (Self::DimmingCurve { curve, width }, PatchValue::Components(values)) => {
                check_width(*width as usize, values.len())?;
                let [PatchValue::Components(output)] = outputs else {
                    return Err(FilterError::TypeMismatch);
                };
                output.clear();
                output.extend(
                    values
                        .iter()
                        .map(|value| apply_dimming_curve(curve, *value)),
                );
            }
            (
                Self::ScaleInvert {
                    scale,
                    invert,
                    width,
                },
                PatchValue::Components(values),
            ) => {
                check_width(*width as usize, values.len())?;
                let [PatchValue::Components(output)] = outputs else {
                    return Err(FilterError::TypeMismatch);
                };
                output.clear();
                output.extend(values.iter().map(|value| {
                    let value = if *invert {
                        1.0 - value.clamp(0.0, 1.0)
                    } else {
                        *value
                    };
                    (value * scale).clamp(0.0, 1.0)
                }));
            }
            (
                Self::FanOut {
                    width,
                    outputs: output_count,
                },
                PatchValue::Components(values),
            ) => {
                check_width(*width as usize, values.len())?;
                if usize::from(*output_count) != outputs.len() {
                    return Err(FilterError::TypeMismatch);
                }
                for output in outputs {
                    let PatchValue::Components(output) = output else {
                        return Err(FilterError::TypeMismatch);
                    };
                    output.clone_from(values);
                }
            }
            (
                Self::ComponentReorder {
                    components_per_cell,
                    order,
                    cell_count,
                },
                PatchValue::Components(values),
            ) => {
                let per_cell = usize::from(*components_per_cell);
                check_width(per_cell * *cell_count as usize, values.len())?;
                let [PatchValue::Components(output)] = outputs else {
                    return Err(FilterError::TypeMismatch);
                };
                output.clear();
                for cell in values.chunks_exact(per_cell) {
                    output.extend(order.iter().map(|component| cell[usize::from(*component)]));
                }
            }
            (Self::IndexedValueMapping { entries, width }, PatchValue::Indexed(values)) => {
                check_width(*width as usize, values.len())?;
                let [PatchValue::Components(output)] = outputs else {
                    return Err(FilterError::TypeMismatch);
                };
                output.clear();
                for value in values {
                    output.push(
                        entries
                            .binary_search_by_key(value, |(id, _)| *id)
                            .ok()
                            .map(|index| entries[index].1)
                            .ok_or(FilterError::MissingIndexedMapping(*value))?,
                    );
                }
            }
            (Self::Quantize8 { width }, PatchValue::Components(values)) => {
                check_width(*width as usize, values.len())?;
                let [PatchValue::Slots(output)] = outputs else {
                    return Err(FilterError::TypeMismatch);
                };
                output.clear();
                output.extend(values.iter().map(|value| quantize8(*value)));
            }
            (Self::Quantize16 { width, byte_order }, PatchValue::Components(values)) => {
                check_width(*width as usize, values.len())?;
                let [PatchValue::Slots(slots)] = outputs else {
                    return Err(FilterError::TypeMismatch);
                };
                slots.clear();
                for value in values {
                    let encoded = quantize16(*value).to_be_bytes();
                    match byte_order {
                        ByteOrder::CoarseFine => slots.extend(encoded),
                        ByteOrder::FineCoarse => slots.extend([encoded[1], encoded[0]]),
                    }
                }
            }
            _ => return Err(FilterError::TypeMismatch),
        }
        Ok(())
    }
}

fn check_width(expected: usize, actual: usize) -> Result<(), FilterError> {
    if expected == actual {
        Ok(())
    } else {
        Err(FilterError::WidthMismatch { expected, actual })
    }
}

#[derive(Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct PatchSource {
    pub spans: Box<[PatchSourceSpan]>,
}

#[derive(Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct PatchSourceSpan {
    pub element: u32,
    pub cells: core::ops::Range<u32>,
}

impl PatchSource {
    pub fn write(
        &self,
        elements: &[RenderedElementState],
        output: &mut PatchValue,
    ) -> Result<(), FilterError> {
        match output {
            PatchValue::Colors(output) => {
                output.clear();
                for span in &self.spans {
                    let values = match elements.get(span.element as usize) {
                        Some(RenderedElementState::Color { cells, .. }) => {
                            cells.get(span.cells.start as usize..span.cells.end as usize)
                        }
                        Some(RenderedElementState::Fixture { color, .. })
                            if span.cells == (0..1) =>
                        {
                            Some(core::slice::from_ref(color))
                        }
                        _ => None,
                    }
                    .ok_or(FilterError::TypeMismatch)?;
                    output.extend_from_slice(values);
                }
            }
            PatchValue::Scalars(output) => {
                output.clear();
                for span in &self.spans {
                    let values = match elements.get(span.element as usize) {
                        Some(RenderedElementState::Scalar { cells, .. }) => {
                            cells.get(span.cells.start as usize..span.cells.end as usize)
                        }
                        _ => None,
                    }
                    .ok_or(FilterError::TypeMismatch)?;
                    output.extend_from_slice(values);
                }
            }
            PatchValue::Indexed(output) => {
                output.clear();
                for span in &self.spans {
                    let values = match elements.get(span.element as usize) {
                        Some(RenderedElementState::Indexed { cells, .. }) => {
                            cells.get(span.cells.start as usize..span.cells.end as usize)
                        }
                        _ => None,
                    }
                    .ok_or(FilterError::TypeMismatch)?;
                    output.extend_from_slice(values);
                }
            }
            PatchValue::FixtureStates(output) => {
                output.resize_with(self.spans.len(), || FixtureState {
                    functions: Vec::new(),
                });
                for (output, span) in output.iter_mut().zip(&self.spans) {
                    let state = match elements.get(span.element as usize) {
                        Some(RenderedElementState::Fixture { state, .. })
                            if span.cells == (0..1) =>
                        {
                            Some(state)
                        }
                        _ => None,
                    }
                    .ok_or(FilterError::TypeMismatch)?;
                    output.functions.clone_from(&state.functions);
                }
            }
            _ => return Err(FilterError::TypeMismatch),
        }
        Ok(())
    }
}

#[derive(Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct PreparedPatch {
    pub steps: Box<[PatchStep]>,
    pub value_layouts: Box<[PatchValueLayout]>,
    pub fixture_programs: Box<[FixtureProgram]>,
}

#[derive(Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum PatchStep {
    Source {
        output: u32,
        source: PatchSource,
    },
    Filter {
        input: u32,
        output_start: u32,
        filter: PreparedFilter,
    },
    Fixture {
        input: u32,
        output_start: u32,
        program: u32,
    },
    Sink {
        input: u32,
        frame: u32,
        start: u32,
        end: u32,
    },
}

#[derive(Debug)]
pub struct PatchWorkspace {
    values: Vec<PatchValue>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PatchError {
    Filter(FilterError),
    Fixture(FixtureEncodingError),
    TypeMismatch,
    WidthMismatch,
}

impl PreparedPatch {
    pub fn workspace(&self) -> PatchWorkspace {
        PatchWorkspace {
            values: self
                .value_layouts
                .iter()
                .copied()
                .map(PatchValue::new)
                .collect(),
        }
    }

    pub fn evaluate(
        &self,
        elements: &[RenderedElementState],
        frames: &mut [impl AsMut<[u8]>],
        workspace: &mut PatchWorkspace,
    ) -> Result<(), PatchError> {
        for frame in frames.iter_mut() {
            frame.as_mut().fill(0);
        }
        for step in &self.steps {
            match step {
                PatchStep::Source { output, source } => {
                    source
                        .write(elements, &mut workspace.values[*output as usize])
                        .map_err(PatchError::Filter)?;
                }
                PatchStep::Filter {
                    input,
                    output_start,
                    ..
                }
                | PatchStep::Fixture {
                    input,
                    output_start,
                    ..
                } => {
                    let (input, output) = if input < output_start {
                        let (before, after) = workspace.values.split_at_mut(*output_start as usize);
                        (&before[*input as usize], &mut after[0])
                    } else {
                        let (before, after) = workspace.values.split_at_mut(*input as usize);
                        (&after[0], &mut before[*output_start as usize])
                    };
                    match step {
                        PatchStep::Fixture { program, .. } => {
                            let program = &self.fixture_programs[*program as usize];
                            let (PatchValue::FixtureStates(states), PatchValue::Slots(output)) =
                                (input, output)
                            else {
                                return Err(PatchError::TypeMismatch);
                            };
                            output.resize(states.len() * program.slot_count as usize, 0);
                            program
                                .encode(states, output)
                                .map_err(PatchError::Fixture)?;
                        }
                        PatchStep::Filter { filter, .. } => {
                            filter
                                .evaluate(input, core::slice::from_mut(output))
                                .map_err(PatchError::Filter)?;
                        }
                        _ => unreachable!(),
                    }
                }
                PatchStep::Sink {
                    input,
                    frame,
                    start,
                    end,
                } => {
                    let PatchValue::Slots(slots) = &workspace.values[*input as usize] else {
                        return Err(PatchError::TypeMismatch);
                    };
                    if slots.len() != (end - start) as usize {
                        return Err(PatchError::WidthMismatch);
                    }
                    frames[*frame as usize].as_mut()[*start as usize..*end as usize]
                        .copy_from_slice(slots);
                }
            }
        }
        Ok(())
    }
}
