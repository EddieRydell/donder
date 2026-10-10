//! Automation admitted against a program's parameter layout.
//!
//! A plan writes the parameter copy it was detached into: words directly, and
//! curve windows in place, so the copy's windows must not be shared.

use super::parameters::{BoundParams, ResourceParam};
use super::{Arc, PreparedCurve};
use crate::sampling::{curve_area, sample_curve};
use crate::signal::PreparedAutomation;
use alloc::{boxed::Box, vec::Vec};
use donder_runtime_types::bytecode::{BytecodeProgram, param_bank};
use donder_runtime_types::{AutomatedQuantity, AutomationMapping};
use donder_runtime_types::{Curve, SECONDS_PER_TICK, SampleDuration, SampleTime};

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

/// Words and resources are banks of the program's parameter layout.
#[derive(Clone, Debug)]
enum Destination {
    Float {
        word: usize,
        min: f32,
        max: f32,
    },
    FloatIntegral {
        word: usize,
        min: f32,
        max: f32,
    },
    Int {
        word: usize,
        min: i32,
        max: i32,
    },
    Bool {
        word: usize,
    },
    /// The options' indices in the program's names, in mapping order.
    Enum {
        word: usize,
        options: Box<[i32]>,
    },
    Curve {
        window: usize,
        min: f32,
        max: f32,
    },
}

/// A curve parameter that bindings rewrite, and the most points they write.
#[derive(Clone, Debug)]
struct Window {
    resource: usize,
    point_capacity: usize,
}

impl AutomationPlan {
    pub(crate) fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    /// Metadata retained by one plan. Shared authored curves retain only
    /// handles here; detached windows are budgeted by
    /// `BoundParams::automation_storage_estimate`.
    pub(crate) fn storage_estimate(bindings: &[PreparedAutomation]) -> Option<usize> {
        let mut bytes =
            size_of::<Self>().checked_add(bindings.len().checked_mul(size_of::<Binding>())?)?;
        for binding in bindings {
            let extra = match &binding.mapping {
                AutomationMapping::Enum { values } => values.len().checked_mul(size_of::<i32>())?,
                // Admission shares a window for repeated curve destinations.
                // Counting one per binding is a conservative allocation bound.
                AutomationMapping::Curve { .. } => size_of::<Window>(),
                _ => 0,
            };
            bytes = bytes.checked_add(extra)?;
        }
        Some(bytes)
    }

    /// The curve resources this plan rewrites, each with the most points
    /// written to it.
    pub(crate) fn windows(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        self.windows
            .iter()
            .map(|window| (window.resource, window.point_capacity))
    }

    /// Reconstruct authored bindings, not the current contents of windows.
    pub(crate) fn to_raw(&self, program: &BytecodeProgram) -> Box<[PreparedAutomation]> {
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
                    Destination::Enum { options, .. } => AutomationMapping::Enum {
                        values: options
                            .iter()
                            .map(|&index| program.enums[index as usize].clone())
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

    /// Materialize automation already checked with the language invocation
    /// of `program`.
    pub(crate) fn from_accepted(
        program: &BytecodeProgram,
        bindings: &[PreparedAutomation],
        origin: SampleTime,
    ) -> Self {
        let mut admitted = Vec::with_capacity(bindings.len());
        let mut windows: Vec<Window> = Vec::new();
        for binding in bindings {
            let bank = usize::from(param_bank(
                &program.params,
                usize::from(binding.param_index),
            ));
            let destination = match (&binding.mapping, binding.quantity) {
                (AutomationMapping::Float { min, max }, AutomatedQuantity::Integral) => {
                    Destination::FloatIntegral {
                        word: bank,
                        min: *min,
                        max: *max,
                    }
                }
                (_, AutomatedQuantity::Integral) => {
                    unreachable!("admission accepts integrals of float automation only")
                }
                (mapping, AutomatedQuantity::Value) => match mapping {
                    AutomationMapping::Float { min, max } => Destination::Float {
                        word: bank,
                        min: *min,
                        max: *max,
                    },
                    AutomationMapping::Int { min, max } => Destination::Int {
                        word: bank,
                        min: *min,
                        max: *max,
                    },
                    AutomationMapping::Bool => Destination::Bool { word: bank },
                    AutomationMapping::Enum { values } => Destination::Enum {
                        word: bank,
                        // Lowering names every option of a bound enum.
                        options: values
                            .iter()
                            .map(|value| {
                                program
                                    .enums
                                    .iter()
                                    .position(|option| option == value)
                                    .map_or(-1, |index| index as i32)
                            })
                            .collect(),
                    },
                    AutomationMapping::Curve { min, max } => {
                        let points = binding.curve.points.len().max(1);
                        let window = match windows.iter().position(|window| window.resource == bank)
                        {
                            Some(index) => {
                                let window = &mut windows[index];
                                window.point_capacity = window.point_capacity.max(points);
                                index
                            }
                            None => {
                                windows.push(Window {
                                    resource: bank,
                                    point_capacity: points,
                                });
                                windows.len() - 1
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

    /// A copy of `params` this plan may write: each window is its own curve
    /// with room for every point a binding writes, so `apply` allocates
    /// nothing.
    pub(crate) fn detach(&self, params: &BoundParams) -> BoundParams {
        let mut params = params.clone();
        for window in &self.windows {
            let ResourceParam::Curve(curve) = &mut params.resources[window.resource] else {
                unreachable!("admission binds curve windows to curve parameters")
            };
            let mut detached: PreparedCurve = curve.detached_clone();
            detached.reserve_window_capacity(window.point_capacity);
            *curve = Arc::new(detached);
        }
        params
    }

    /// Apply in authored order, preserving last-binding-wins behavior, to
    /// this plan's detached copy.
    pub(crate) fn apply(&self, params: &mut BoundParams, time: SampleTime) {
        for binding in &self.bindings {
            let elapsed = time.as_ticks().saturating_sub(binding.start.as_ticks());
            let position = (elapsed as f32 / binding.duration.as_ticks() as f32).clamp(0.0, 1.0);
            let amount = || sample_curve(&binding.curve, position).clamp(0.0, 1.0);
            match &binding.destination {
                Destination::Float { word, min, max } => {
                    params.words[*word] = (min + (max - min) * amount()).to_bits();
                }
                Destination::FloatIntegral { word, min, max } => {
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
                        * SECONDS_PER_TICK;
                    let seconds = ticks * SECONDS_PER_TICK;
                    params.words[*word] = (min * elapsed + (max - min) * seconds * area).to_bits();
                }
                Destination::Int { word, min, max } => {
                    let (min, max) = (*min as f32, *max as f32);
                    params.words[*word] = libm::roundf(min + (max - min) * amount()) as i32 as u32;
                }
                Destination::Bool { word } => {
                    params.words[*word] = u32::from(amount() >= 0.5);
                }
                Destination::Enum { word, options } => {
                    let index = (libm::floorf(amount() * options.len() as f32) as usize)
                        .min(options.len() - 1);
                    params.words[*word] = options[index] as u32;
                }
                Destination::Curve { window, min, max } => {
                    let window = &self.windows[*window];
                    let ResourceParam::Curve(curve) = &mut params.resources[window.resource] else {
                        unreachable!("admission binds curve windows to curve parameters")
                    };
                    Arc::make_mut(curve).update_window(&binding.curve, *min, *max, position);
                }
            }
        }
    }
}
