//! Routing a console's inputs to the commands that start and stop its voices,
//! add live marks and set live automations. The routing is the show's; the
//! state is the playback's: which copy each voice last started and for how
//! long, each set parameter's last value, each axis's position, and which
//! presses are held. Every copy plays for a length; a held copy starts again
//! each time its length ends, until it is stopped.
use alloc::vec;
use alloc::vec::Vec;
use donder_runtime_types::{ConsoleEvent, InputEvent, InputHappening, SampleDuration, SampleTime};

use crate::format;
use crate::playback::ConsoleCommand;
use crate::view::Show;

/// A copy of a clip that started playing.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Copy {
    /// When its current pass started.
    started: SampleTime,
    length: SampleDuration,
    /// Whether it starts again when its length ends, until stopped.
    held: bool,
}

impl Copy {
    fn plays_at(self, time: SampleTime) -> bool {
        self.held
            || time.as_ticks().saturating_sub(self.started.as_ticks()) < self.length.as_ticks()
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
    /// The value each live automation was last set to; none until written, so
    /// the first write is always applied.
    values: Vec<f32>,
    held: Vec<Press>,
}

impl Router {
    pub(crate) fn new(show: Show<'_>) -> Self {
        Self {
            copies: vec![None; show.s.voices.len()],
            axes: vec![0.0; show.meta().controls as usize],
            values: vec![f32::NAN; show.s.live_automations.len()],
            held: Vec::new(),
        }
    }

    /// Start the session at `time`: each `always` clip starts as a `Start`
    /// would.
    pub(crate) fn start(
        &mut self,
        show: Show<'_>,
        time: SampleTime,
        apply: &mut impl FnMut(ConsoleCommand),
    ) {
        for action in show.s.console_always {
            self.act(show, action, None, 1.0, time, apply);
        }
    }

    /// Start again each held copy whose length ended by `time`, from the
    /// start of the pass `time` falls in.
    pub(crate) fn advance(
        &mut self,
        time: SampleTime,
        apply: &mut impl FnMut(SampleTime, ConsoleCommand),
    ) {
        for (voice, copy) in self.copies.iter_mut().enumerate() {
            let Some(copy) = copy.as_mut().filter(|copy| copy.held) else {
                continue;
            };
            let length = copy.length.as_ticks();
            let elapsed = time.as_ticks().saturating_sub(copy.started.as_ticks());
            if elapsed < length {
                continue;
            }
            copy.started = SampleTime::from_ticks(time.as_ticks() - elapsed % length);
            apply(
                copy.started,
                ConsoleCommand::Start {
                    voice: voice as u16,
                    duration: copy.length,
                },
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
                self.act(show, action, note, value, event.time, apply);
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
        note: Option<u8>,
        value: f32,
        time: SampleTime,
        apply: &mut impl FnMut(ConsoleCommand),
    ) {
        let clip = action.target;
        let scale = match action.length {
            format::LENGTH_VELOCITY => value,
            format::LENGTH_AXIS => self.axes[action.axis as usize],
            _ => 0.0,
        };
        let span = (action.longest - action.shortest) as f32 * scale;
        let duration = SampleDuration::from_ticks(action.shortest + (span + 0.5) as u32);
        match action.kind {
            format::ACTION_PLAY => {
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
                    show, clip, position, duration, false, value, note, time, apply,
                );
            }
            format::ACTION_START => {
                if !self.plays(show, clip, time) {
                    self.start_copy(show, clip, 0, duration, true, value, note, time, apply);
                }
            }
            format::ACTION_STOP => self.stop(show, clip, time, apply),
            format::ACTION_TOGGLE => {
                if self.plays(show, clip, time) {
                    self.stop(show, clip, time, apply);
                } else {
                    self.start_copy(show, clip, 0, duration, true, value, note, time, apply);
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

    /// Start the copy at `position` of `clip` for `duration`, repeating
    /// until stopped when `held`, with the values its press played.
    #[allow(clippy::too_many_arguments)]
    fn start_copy(
        &mut self,
        show: Show<'_>,
        clip: u32,
        position: usize,
        duration: SampleDuration,
        held: bool,
        velocity: f32,
        note: Option<u8>,
        time: SampleTime,
        apply: &mut impl FnMut(ConsoleCommand),
    ) {
        let voice = Self::voices(show, clip)[position];
        self.copies[voice as usize] = Some(Copy {
            started: time,
            length: duration,
            held,
        });
        apply(ConsoleCommand::Start {
            voice: voice as u16,
            duration,
        });
        // Values follow the start, so a copy plays with its own from the
        // frame it starts.
        let values = show.s.console_clips[clip as usize].values;
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

/// A played value, in `0..=1`, from a press at `velocity` of `note`, or of
/// no note.
fn played(source: u32, velocity: f32, note: Option<u8>) -> f32 {
    match (source, note) {
        (format::PLAYED_PITCH_CLASS, Some(note)) => f32::from(note % 12) / 12.0,
        // Validation keeps note sources off buttons without notes.
        (format::PLAYED_PITCH_CLASS, None) => 0.0,
        _ => velocity,
    }
}
