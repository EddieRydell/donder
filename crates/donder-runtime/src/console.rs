//! Routing a console's inputs to the commands that start and stop its voices,
//! add live marks and set live automations. The routing is the show's; the
//! state is the playback's: which copy each voice last started and until when,
//! each set parameter's last value, each axis's position, and which presses
//! are held.
use alloc::vec;
use alloc::vec::Vec;
use donder_runtime_types::{ConsoleEvent, InputEvent, InputHappening, SampleDuration, SampleTime};

use crate::format::{self, NONE};
use crate::playback::ConsoleCommand;
use crate::view::Show;

/// A copy of a clip that started playing.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Copy {
    started: SampleTime,
    /// `None` plays until stopped.
    ends: Option<SampleTime>,
}

impl Copy {
    fn plays_at(self, time: SampleTime) -> bool {
        self.ends.is_none_or(|end| time < end)
    }
}

/// One press: a control, and its note when it reads notes.
type Press = (u16, Option<u8>);

/// Mutable routing state for one session.
#[derive(Clone, Debug)]
pub(crate) struct Router {
    /// The copy each voice last started, if any.
    copies: Vec<Option<Copy>>,
    /// Where each control's axis is now; an axis not yet moved is at 0.
    axes: Vec<f32>,
    /// The value each live automation was last set to; a parameter holds its
    /// own value until written, so the first write is always applied.
    values: Vec<f32>,
    held: Vec<Press>,
}

impl Router {
    pub(crate) fn new(show: Show<'_>) -> Self {
        Self {
            copies: vec![None; show.s.voices.len()],
            axes: vec![0.0; show.s.console_controls.len()],
            values: vec![f32::NAN; show.s.live_automations.len()],
            held: Vec::new(),
        }
    }

    /// Start the session at `time`: each `always` clip starts and plays until
    /// the session ends.
    pub(crate) fn start(
        &mut self,
        show: Show<'_>,
        time: SampleTime,
        apply: &mut impl FnMut(ConsoleCommand),
    ) {
        for &clip in show.s.console_always {
            self.start_copy(
                show,
                clip,
                0,
                None,
                format::Range::EMPTY,
                1.0,
                None,
                time,
                apply,
            );
        }
    }

    /// Route `event`. A press of a control already held, as a computer key
    /// repeats, and a release of one not held do nothing.
    pub(crate) fn route(
        &mut self,
        show: Show<'_>,
        event: ConsoleEvent,
        apply: &mut impl FnMut(ConsoleCommand),
    ) {
        let InputEvent {
            control,
            note,
            happening,
            value,
        } = event.input;
        if usize::from(control) >= self.axes.len() {
            return;
        }
        let value = value.clamp(0.0, 1.0);
        let press = (control, note);
        match happening {
            InputHappening::Press if self.held.contains(&press) => return,
            InputHappening::Press => self.held.push(press),
            InputHappening::Release => {
                let Some(held) = self.held.iter().position(|held| *held == press) else {
                    return;
                };
                self.held.swap_remove(held);
            }
            InputHappening::Move => self.axes[usize::from(control)] = value,
        }
        let happening = match happening {
            InputHappening::Press => format::HAPPENING_PRESS,
            InputHappening::Release => format::HAPPENING_RELEASE,
            InputHappening::Move => format::HAPPENING_MOVE,
        };
        for on in show
            .s
            .console_ons
            .iter()
            .filter(|on| on.control == u32::from(control) && on.happening == happening)
        {
            for action in on.actions.get(show.s.console_actions) {
                self.act(show, action, press, value, event.time, apply);
            }
        }
    }

    /// Release every held press, as when its input goes away.
    pub(crate) fn release_all(
        &mut self,
        show: Show<'_>,
        time: SampleTime,
        apply: &mut impl FnMut(ConsoleCommand),
    ) {
        for (control, note) in self.held.clone() {
            let input = InputEvent {
                control,
                note,
                happening: InputHappening::Release,
                value: 0.0,
            };
            self.route(show, ConsoleEvent { time, input }, apply);
        }
    }

    fn act(
        &mut self,
        show: Show<'_>,
        action: &format::ConsoleAction,
        (control, note): Press,
        value: f32,
        time: SampleTime,
        apply: &mut impl FnMut(ConsoleCommand),
    ) {
        let note = note.map(|note| (note, show.s.console_controls[usize::from(control)]));
        let clip = action.target;
        match action.kind {
            format::ACTION_PLAY => {
                let scale = match action.length {
                    format::LENGTH_VELOCITY => value,
                    format::LENGTH_AXIS => self.axes[action.axis as usize],
                    _ => 0.0,
                };
                let span = (action.longest - action.shortest) as f32 * scale;
                let duration = SampleDuration::from_ticks(action.shortest + (span + 0.5) as u32);
                // A free copy, or else the oldest of the action's copies.
                let voices = Self::voices(show, clip);
                let copies = voices[..(action.copies as usize).min(voices.len())]
                    .iter()
                    .map(|&voice| self.copies[voice as usize]);
                let position = copies
                    .clone()
                    .position(|copy| copy.is_none_or(|copy| !copy.plays_at(time)))
                    .unwrap_or_else(|| {
                        copies
                            .enumerate()
                            .min_by_key(|(_, copy)| copy.map(|copy| copy.started))
                            .map_or(0, |(position, _)| position)
                    });
                self.start_copy(
                    show,
                    clip,
                    position,
                    Some(duration),
                    action.values,
                    value,
                    note,
                    time,
                    apply,
                );
            }
            format::ACTION_START => {
                if !self.plays(show, clip, time) {
                    self.start_copy(show, clip, 0, None, action.values, value, note, time, apply);
                }
            }
            format::ACTION_STOP => self.stop(show, clip, time, apply),
            format::ACTION_TOGGLE => {
                if self.plays(show, clip, time) {
                    self.stop(show, clip, time, apply);
                } else {
                    self.start_copy(show, clip, 0, None, action.values, value, note, time, apply);
                }
            }
            format::ACTION_MARK => apply(ConsoleCommand::Mark {
                collection: action.target as u16,
            }),
            _ => {
                let written = action.low + (action.high - action.low) * value;
                let last = &mut self.values[action.target as usize];
                if *last != written {
                    *last = written;
                    apply(ConsoleCommand::Set {
                        automation: action.target as u16,
                        value: written,
                    });
                }
            }
        }
    }

    /// The voices of console clip `clip`, in voice indices.
    fn voices<'a>(show: Show<'a>, clip: u32) -> &'a [u32] {
        show.s.console_clips[clip as usize]
            .voices
            .get(show.s.console_indices)
    }

    fn plays(&self, show: Show<'_>, clip: u32, time: SampleTime) -> bool {
        Self::voices(show, clip)
            .iter()
            .any(|&voice| self.copies[voice as usize].is_some_and(|copy| copy.plays_at(time)))
    }

    /// Start the copy at `position` of `clip`, for `duration` or until
    /// stopped, with the values played on it.
    #[allow(clippy::too_many_arguments)]
    fn start_copy(
        &mut self,
        show: Show<'_>,
        clip: u32,
        position: usize,
        duration: Option<SampleDuration>,
        values: format::Range,
        velocity: f32,
        note: Option<(u8, format::ConsoleControl)>,
        time: SampleTime,
        apply: &mut impl FnMut(ConsoleCommand),
    ) {
        let voice = Self::voices(show, clip)[position];
        self.copies[voice as usize] = Some(Copy {
            started: time,
            ends: duration.and_then(|duration| time.checked_add_duration(duration)),
        });
        apply(ConsoleCommand::Start {
            voice: voice as u16,
            duration,
        });
        // Values follow the start, so a copy plays with its own from the
        // frame it starts.
        for value in values.get(show.s.played_values) {
            let automation = value.automations.get(show.s.console_indices)[position];
            apply(ConsoleCommand::Set {
                automation: automation as u16,
                value: value.low + (value.high - value.low) * played(value.source, velocity, note),
            });
        }
    }

    /// Stop every copy of `clip` that plays at `time`.
    fn stop(
        &mut self,
        show: Show<'_>,
        clip: u32,
        time: SampleTime,
        apply: &mut impl FnMut(ConsoleCommand),
    ) {
        for &voice in Self::voices(show, clip) {
            let copy = &mut self.copies[voice as usize];
            if copy.is_some_and(|copy| copy.plays_at(time)) {
                apply(ConsoleCommand::Stop {
                    voice: voice as u16,
                });
            }
            *copy = None;
        }
    }
}

/// A played value, in `0..=1`, from a press at `velocity` of `note` of a
/// control, or of no note.
fn played(source: u32, velocity: f32, note: Option<(u8, format::ConsoleControl)>) -> f32 {
    let Some((note, control)) = note else {
        // Validation keeps note sources off buttons without notes.
        return if source == format::PLAYED_VELOCITY {
            velocity
        } else {
            0.0
        };
    };
    match source {
        format::PLAYED_VELOCITY => velocity,
        format::PLAYED_PITCH => {
            if control.low != NONE && control.high > control.low {
                (u32::from(note).saturating_sub(control.low)) as f32
                    / (control.high - control.low) as f32
            } else {
                // A button of one note is at its place among all 128 notes.
                f32::from(note) / 127.0
            }
        }
        format::PLAYED_PITCH_CLASS => f32::from(note % 12) / 12.0,
        _ => f32::from((u16::from(note) * 7 % 12) as u8) / 12.0,
    }
}
