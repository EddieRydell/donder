use crate::element::{
    ElementCellAddress, ElementNodeKind, ElementSelection, ElementSelectionError, ElementTree,
    IndexedOptionId,
};
use crate::fixture_profile::{
    FixtureEntryId, FixtureFunctionId, FixtureFunctionKind, FixtureProfileStore,
};
use crate::values::{Color, Curve, DawnDuration, DawnTime, Gradient};

pub mod authoring;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ControlClipId(pub u32);

#[derive(Clone, Debug, PartialEq)]
pub struct ControlClip {
    pub id: ControlClipId,
    pub start: DawnTime,
    pub duration: DawnDuration,
    pub target: ControlTarget,
    pub value: ControlValue,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum ControlTarget {
    Scalar(ElementSelection),
    Indexed(ElementSelection),
    FixtureFunction {
        selection: ElementSelection,
        function: FixtureFunctionId,
    },
}

impl ControlTarget {
    pub fn selection_mut(&mut self) -> &mut ElementSelection {
        match self {
            Self::Scalar(selection) | Self::Indexed(selection) => selection,
            Self::FixtureFunction { selection, .. } => selection,
        }
    }

    pub fn selection(&self) -> &ElementSelection {
        match self {
            Self::Scalar(selection) | Self::Indexed(selection) => selection,
            Self::FixtureFunction { selection, .. } => selection,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ControlValue {
    ConstantNormalized(f32),
    NormalizedCurve(Curve),
    Indexed {
        option: IndexedOptionId,
        range_curve: Option<Curve>,
    },
    FixtureIndexed {
        entry: FixtureEntryId,
        range_curve: Option<Curve>,
    },
    ConstantColor(Color),
    Gradient(Gradient),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ControlValidationError {
    EmptyDuration(ControlClipId),
    InvalidNormalizedValue(ControlClipId),
    InvalidCurve(ControlClipId),
    TypeMismatch(ControlClipId),
    Conflict {
        first: ControlClipId,
        second: ControlClipId,
    },
}

pub fn validate_control_clip(clip: &ControlClip) -> Result<(), ControlValidationError> {
    if clip.duration.0.is_zero() {
        return Err(ControlValidationError::EmptyDuration(clip.id));
    }
    match &clip.value {
        ControlValue::ConstantNormalized(value)
            if !value.is_finite() || !(0.0..=1.0).contains(value) =>
        {
            Err(ControlValidationError::InvalidNormalizedValue(clip.id))
        }
        ControlValue::NormalizedCurve(curve)
        | ControlValue::Indexed {
            range_curve: Some(curve),
            ..
        }
        | ControlValue::FixtureIndexed {
            range_curve: Some(curve),
            ..
        } if !valid_curve(curve) => Err(ControlValidationError::InvalidCurve(clip.id)),
        ControlValue::Gradient(gradient) if gradient.validate().is_err() => {
            Err(ControlValidationError::InvalidCurve(clip.id))
        }
        _ => Ok(()),
    }
}

fn times_overlap(left: &ControlClip, right: &ControlClip) -> bool {
    let left_start = left.start.0;
    let right_start = right.start.0;
    let left_end = left_start.saturating_add(left.duration.0);
    let right_end = right_start.saturating_add(right.duration.0);
    left_start < right_end && right_start < left_end
}

#[derive(Clone, Debug, PartialEq)]
pub enum ControlTargetError {
    Selection(ElementSelectionError),
    EmptySelection,
    TypeMismatch,
    MissingOption(IndexedOptionId),
    MissingFunction(FixtureFunctionId),
    MissingProfile(crate::fixture_profile::FixtureProfileId),
    MissingEntry(FixtureEntryId),
    UnsupportedRange,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ControlResolutionError {
    InvalidClip(ControlValidationError),
    InvalidTarget {
        clip: ControlClipId,
        reason: ControlTargetError,
    },
    Conflict {
        first: ControlClipId,
        second: ControlClipId,
        address: ElementCellAddress,
    },
}

impl std::fmt::Display for ControlResolutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidClip(error) => write!(f, "Invalid control clip: {error:?}"),
            Self::InvalidTarget { clip, reason } => {
                write!(f, "Control clip {}: ", clip.0)?;
                match reason {
                    ControlTargetError::Selection(error) => {
                        write!(f, "invalid element selection: {error:?}")
                    }
                    ControlTargetError::EmptySelection => {
                        write!(f, "choose at least one target cell.")
                    }
                    ControlTargetError::TypeMismatch => write!(
                        f,
                        "the value type does not match every selected element or fixture function."
                    ),
                    ControlTargetError::MissingOption(option) => write!(
                        f,
                        "selected element has no option {}. Retarget the clip before removing that option.",
                        option.0
                    ),
                    ControlTargetError::MissingFunction(function) => write!(
                        f,
                        "fixture function {} is missing. Retarget the clip before changing the profile.",
                        function.0
                    ),
                    ControlTargetError::MissingProfile(profile) => {
                        write!(f, "fixture profile {} is missing.", profile.0.object())
                    }
                    ControlTargetError::MissingEntry(entry) => write!(
                        f,
                        "fixture entry {} is missing. Choose an entry in the selected function.",
                        entry.0
                    ),
                    ControlTargetError::UnsupportedRange => {
                        write!(f, "this indexed option does not support a range curve.")
                    }
                }
            }
            Self::Conflict {
                first,
                second,
                address,
            } => write!(
                f,
                "Control clips {} and {} overlap on element {} cell {}. Use different times, cells, or fixture functions.",
                first.0, second.0, address.node.0, address.cell
            ),
        }
    }
}

pub fn resolve_controls(
    tree: &ElementTree,
    profiles: &FixtureProfileStore,
    clips: &[ControlClip],
) -> Result<Vec<Vec<ElementCellAddress>>, ControlResolutionError> {
    let mut resolved = Vec::with_capacity(clips.len());
    for clip in clips {
        validate_control_clip(clip).map_err(ControlResolutionError::InvalidClip)?;
        let invalid = |reason| ControlResolutionError::InvalidTarget {
            clip: clip.id,
            reason,
        };
        let addresses = tree
            .flatten_selection(clip.target.selection())
            .map_err(|error| invalid(ControlTargetError::Selection(error)))?;
        if addresses.is_empty() {
            return Err(invalid(ControlTargetError::EmptySelection));
        }
        for address in &addresses {
            let node = &tree.nodes[&address.node];
            match (&clip.target, &node.kind, &clip.value) {
                (
                    ControlTarget::Scalar(_),
                    ElementNodeKind::Scalar { .. },
                    ControlValue::ConstantNormalized(_) | ControlValue::NormalizedCurve(_),
                ) => {}
                (
                    ControlTarget::Indexed(_),
                    ElementNodeKind::Indexed { options, .. },
                    ControlValue::Indexed {
                        option,
                        range_curve,
                    },
                ) => {
                    if !options.iter().any(|candidate| candidate.id == *option) {
                        return Err(invalid(ControlTargetError::MissingOption(*option)));
                    }
                    if range_curve.is_some() {
                        return Err(invalid(ControlTargetError::UnsupportedRange));
                    }
                }
                (
                    ControlTarget::FixtureFunction { function, .. },
                    ElementNodeKind::Fixture { profile },
                    value,
                ) => {
                    let definition = profiles
                        .definitions
                        .get(profile)
                        .and_then(|profile| profile.functions.get(function))
                        .ok_or_else(|| invalid(ControlTargetError::MissingFunction(*function)))?;
                    match (&definition.kind, value) {
                        (
                            FixtureFunctionKind::Range,
                            ControlValue::ConstantNormalized(_) | ControlValue::NormalizedCurve(_),
                        )
                        | (
                            FixtureFunctionKind::ColorMixing { .. },
                            ControlValue::ConstantColor(_) | ControlValue::Gradient(_),
                        ) => {}
                        (
                            FixtureFunctionKind::Indexed { entries }
                            | FixtureFunctionKind::ColorWheel { entries },
                            ControlValue::FixtureIndexed { entry, range_curve },
                        ) => {
                            let selected = entries
                                .iter()
                                .find(|candidate| candidate.id == *entry)
                                .ok_or_else(|| invalid(ControlTargetError::MissingEntry(*entry)))?;
                            if range_curve.is_some() && !selected.curve_control {
                                return Err(invalid(ControlTargetError::UnsupportedRange));
                            }
                        }
                        _ => return Err(invalid(ControlTargetError::TypeMismatch)),
                    }
                }
                _ => return Err(invalid(ControlTargetError::TypeMismatch)),
            }
        }
        resolved.push(addresses);
    }
    for (index, left) in clips.iter().enumerate() {
        for (right_index, right) in clips.iter().enumerate().skip(index + 1) {
            let function = |target: &ControlTarget| match target {
                ControlTarget::FixtureFunction { function, .. } => Some(*function),
                _ => None,
            };
            if times_overlap(left, right)
                && function(&left.target) == function(&right.target)
                && let Some(address) = resolved[index]
                    .iter()
                    .find(|address| resolved[right_index].contains(address))
            {
                return Err(ControlResolutionError::Conflict {
                    first: left.id,
                    second: right.id,
                    address: *address,
                });
            }
        }
    }
    Ok(resolved)
}

fn valid_curve(curve: &Curve) -> bool {
    curve.validate().is_ok()
        && curve
            .points
            .iter()
            .all(|point| (0.0..=1.0).contains(&point.value))
}
