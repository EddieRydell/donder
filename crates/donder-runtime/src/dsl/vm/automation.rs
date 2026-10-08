//! Automation admitted against a fixed parameter-bank layout.
//!
//! Keep a plan with the parameter workspace it was admitted for. Curve windows
//! belong to the plan, not to a tagged parameter value supplied during playback.

use super::{Arc, BoundParams, CurveParameter, Identifier, PreparedCurve};
use crate::sampling::{curve_area, sample_curve};
use crate::signal::PreparedAutomation;
use alloc::{boxed::Box, vec::Vec};
use donder_runtime_types::AutomatedQuantity;
use donder_runtime_types::AutomationMapping;
use donder_runtime_types::{Curve, MICROS_PER_SECOND, SampleDuration, SampleTime};

#[derive(Clone, Debug)]
pub(crate) struct AutomationPlan {
    bindings: Box<[Binding]>,
    windows: Box<[Window]>,
    /// Where the program's time starts: its clip's start, or the sequence's.
    origin: SampleTime,
}

impl Default for AutomationPlan {
    fn default() -> Self {
        Self {
            bindings: Box::default(),
            windows: Box::default(),
            origin: SampleTime::from_ticks(0),
        }
    }
}

#[derive(Clone, Debug)]
struct Binding {
    parameter: u16,
    start: SampleTime,
    duration: SampleDuration,
    curve: Arc<Curve>,
    destination: Destination,
}

#[derive(Clone, Debug)]
enum Destination {
    Float {
        slot: usize,
        min: f32,
        max: f32,
    },
    FloatIntegral {
        slot: usize,
        min: f32,
        max: f32,
    },
    Int {
        slot: usize,
        min: i32,
        max: i32,
    },
    Bool {
        slot: usize,
    },
    Enum {
        slot: usize,
        first: Identifier,
        rest: Box<[Identifier]>,
    },
    Curve {
        window: usize,
        min: f32,
        max: f32,
    },
}

#[derive(Debug)]
struct Window {
    slot: usize,
    curve: Arc<PreparedCurve>,
    point_capacity: usize,
}

impl Clone for Window {
    fn clone(&self) -> Self {
        let mut curve = self.curve.detached_clone();
        // Vec::clone need not retain spare capacity. A cloned playback plan
        // must still accommodate every authored window without growing.
        curve.reserve_window_capacity(self.point_capacity);
        Self {
            slot: self.slot,
            curve: Arc::new(curve),
            point_capacity: self.point_capacity,
        }
    }
}

impl AutomationPlan {
    pub(crate) fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    /// Metadata retained by one plan. Shared authored curves/identifiers retain
    /// only handles here; their backing allocations belong to the raw data.
    /// Detached curve allocations are budgeted by `automation_storage_estimate`.
    pub(crate) fn storage_estimate(bindings: &[PreparedAutomation]) -> Option<usize> {
        let mut bytes =
            size_of::<Self>().checked_add(bindings.len().checked_mul(size_of::<Binding>())?)?;
        for binding in bindings {
            let extra = match &binding.mapping {
                AutomationMapping::Enum { values } => values
                    .iter()
                    .skip(1)
                    .len()
                    .checked_mul(size_of::<Identifier>())?,
                // Admission shares a window for repeated curve destinations.
                // Counting one per binding is a conservative allocation bound.
                AutomationMapping::Curve { .. } => size_of::<Window>(),
                _ => 0,
            };
            bytes = bytes.checked_add(extra)?;
        }
        Some(bytes)
    }

    /// Reconstruct authored bindings, not the current contents of mutable windows.
    pub(crate) fn to_raw(&self) -> Box<[PreparedAutomation]> {
        self.bindings
            .iter()
            .map(|binding| PreparedAutomation {
                param_index: binding.parameter,
                start: binding.start,
                duration: binding.duration,
                curve: Arc::clone(&binding.curve),
                quantity: match binding.destination {
                    Destination::FloatIntegral { .. } => AutomatedQuantity::Integral,
                    _ => AutomatedQuantity::Value,
                },
                mapping: match &binding.destination {
                    Destination::Float { min, max, .. }
                    | Destination::FloatIntegral { min, max, .. } => AutomationMapping::Float {
                        min: *min,
                        max: *max,
                    },
                    Destination::Int { min, max, .. } => AutomationMapping::Int {
                        min: *min,
                        max: *max,
                    },
                    Destination::Bool { .. } => AutomationMapping::Bool,
                    Destination::Enum { first, rest, .. } => AutomationMapping::Enum {
                        values: core::iter::once(first)
                            .chain(rest.iter())
                            .cloned()
                            .collect(),
                    },
                    Destination::Curve { min, max, .. } => AutomationMapping::Curve {
                        min: *min,
                        max: *max,
                    },
                },
            })
            .collect()
    }

    /// Materialize automation already checked with the language invocation.
    /// The parameter banks must be the materialization of that invocation's inputs.
    pub(crate) fn from_accepted(
        params: &BoundParams,
        bindings: &[PreparedAutomation],
        origin: SampleTime,
    ) -> Self {
        use crate::dsl::bytecode::ParameterKind;
        let values = &params.values;
        let mut admitted = Vec::with_capacity(bindings.len());
        let mut windows: Vec<Window> = Vec::new();
        for binding in bindings {
            let parameter = usize::from(binding.param_index);
            // Parameter indices are declaration-order indices; typed banks retain
            // the same relative order within each kind.
            let kind = ParameterKind::for_type(&params.types()[parameter]);
            let slot = params.types()[..parameter]
                .iter()
                .filter(|ty| ParameterKind::for_type(ty) == kind)
                .count();
            let destination = match (&binding.mapping, binding.quantity) {
                (AutomationMapping::Float { min, max }, AutomatedQuantity::Integral) => {
                    Destination::FloatIntegral {
                        slot,
                        min: *min,
                        max: *max,
                    }
                }
                (_, AutomatedQuantity::Integral) => {
                    unreachable!("admission accepts integrals of float automation only")
                }
                (mapping, AutomatedQuantity::Value) => match mapping {
                    AutomationMapping::Float { min, max } => Destination::Float {
                        slot,
                        min: *min,
                        max: *max,
                    },
                    AutomationMapping::Int { min, max } => Destination::Int {
                        slot,
                        min: *min,
                        max: *max,
                    },
                    AutomationMapping::Bool => Destination::Bool { slot },
                    AutomationMapping::Enum { values: options } => Destination::Enum {
                        slot,
                        first: options[0].clone(),
                        rest: options[1..].into(),
                    },
                    AutomationMapping::Curve { min, max } => {
                        let point_capacity = binding.curve.points.len().max(1);
                        let window = match windows.iter().position(|window| window.slot == slot) {
                            Some(index) => {
                                let window = &mut windows[index];
                                window.point_capacity = window.point_capacity.max(point_capacity);
                                Arc::make_mut(&mut window.curve)
                                    .reserve_window_capacity(window.point_capacity);
                                index
                            }
                            None => {
                                let mut curve = PreparedCurve::new(values.curves[slot].owned());
                                curve.reserve_window_capacity(point_capacity);
                                let index = windows.len();
                                windows.push(Window {
                                    slot,
                                    curve: Arc::new(curve),
                                    point_capacity,
                                });
                                index
                            }
                        };
                        Destination::Curve {
                            window,
                            min: *min,
                            max: *max,
                        }
                    }
                },
            };
            admitted.push(Binding {
                parameter: binding.param_index,
                start: binding.start,
                duration: binding.duration,
                curve: Arc::clone(&binding.curve),
                destination,
            });
        }
        Self {
            bindings: admitted.into(),
            windows: windows.into(),
            origin,
        }
    }

    /// Apply in authored order, preserving last-binding-wins behavior.
    ///
    /// The owner must retain the admitted parameter layout and release forwarded
    /// descendant/VM curve handles before updating an ancestor, as it does for
    /// other prepared curve updates. Plan cloning detaches all mutable windows.
    pub(crate) fn apply(&mut self, params: &mut BoundParams, time: SampleTime) {
        let values = &mut params.values;
        for binding in &self.bindings {
            let elapsed = time.as_ticks().saturating_sub(binding.start.as_ticks());
            let position = (elapsed as f32 / binding.duration.as_ticks() as f32).clamp(0.0, 1.0);
            match &binding.destination {
                Destination::Float { slot, min, max } => {
                    let amount = sample_curve(&binding.curve, position).clamp(0.0, 1.0);
                    values.floats[*slot] = min + (max - min) * amount;
                }
                Destination::FloatIntegral { slot, min, max } => {
                    // Admission keeps automation values in 0..1, so the
                    // value's clamp never applies and the area is exact.
                    let ticks = binding.duration.as_ticks() as f32;
                    let position = |at: SampleTime| {
                        (i64::from(at.as_ticks()) - i64::from(binding.start.as_ticks())) as f32
                            / ticks
                    };
                    let area = curve_area(&binding.curve, position(self.origin), position(time));
                    let elapsed = (i64::from(time.as_ticks()) - i64::from(self.origin.as_ticks()))
                        as f32
                        / MICROS_PER_SECOND as f32;
                    let seconds = ticks / MICROS_PER_SECOND as f32;
                    values.floats[*slot] = min * elapsed + (max - min) * seconds * area;
                }
                Destination::Int { slot, min, max } => {
                    let amount = sample_curve(&binding.curve, position).clamp(0.0, 1.0);
                    let min = *min as f32;
                    let max = *max as f32;
                    values.ints[*slot] = libm::roundf(min + (max - min) * amount) as i32;
                }
                Destination::Bool { slot } => {
                    let amount = sample_curve(&binding.curve, position).clamp(0.0, 1.0);
                    values.bools[*slot] = amount >= 0.5;
                }
                Destination::Enum { slot, first, rest } => {
                    let amount = sample_curve(&binding.curve, position).clamp(0.0, 1.0);
                    let index =
                        (libm::floorf(amount * (rest.len() + 1) as f32) as usize).min(rest.len());
                    let selected = if index == 0 { first } else { &rest[index - 1] };
                    values.enums[*slot].clone_from(selected);
                }
                Destination::Curve { window, min, max } => {
                    let window = &mut self.windows[*window];
                    // Release the previous published handle before mutating the
                    // plan-owned window. This is resource lifetime management,
                    // not a recovery path for an unexpected parameter tag.
                    values.curves[window.slot] = CurveParameter::Empty;
                    Arc::make_mut(&mut window.curve).update_window(
                        &binding.curve,
                        *min,
                        *max,
                        position,
                    );
                    values.curves[window.slot] =
                        CurveParameter::Prepared(Arc::clone(&window.curve));
                }
            }
        }
    }
}
