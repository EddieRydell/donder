//! Playback of a show in a caller's buffer: frames, and the rig commands
//! that start and stop voices, add live marks and set controls.
//!
//! Commands change only the workspace: a voice's start and duration, its
//! layer's start order, live marks, and the controls' parameter values.
use crate::image::{self, NONE};
use donder_runtime_types::{Color, MICROS_PER_SECOND, RigCommand, RigEvent, SampleTime, mul_div};

use crate::dsl::set_value;
use crate::evaluation::{Timeline, evaluate_frame, index_layer, output_buffer};
use crate::show::Show;
use crate::workspace::{LIVE_MARK_CAPACITY, Layout, Timing, Workspace, carve};

/// A show playing in a caller's buffer of [`Layout::words`] words.
pub struct Playback<'a, 'w> {
    show: Show<'a>,
    workspace: Workspace<'w>,
}

impl<'a, 'w> Playback<'a, 'w> {
    /// Start `show` in `buffer`; `None` when it is smaller than the show needs.
    pub fn new(show: Show<'a>, layout: &Layout, buffer: &'w mut [u32]) -> Option<Self> {
        Some(Self {
            show,
            workspace: carve(show, layout, bytemuck::cast_slice_mut(buffer), true)?,
        })
    }

    /// Continue `show` in the `buffer` an earlier playback of it left, as a
    /// caller that keeps the buffer between frames does.
    pub fn resume(show: Show<'a>, layout: &Layout, buffer: &'w mut [u32]) -> Option<Self> {
        Some(Self {
            show,
            workspace: carve(show, layout, bytemuck::cast_slice_mut(buffer), false)?,
        })
    }

    pub fn show(&self) -> Show<'a> {
        self.show
    }

    pub fn evaluate(&mut self, time: SampleTime) -> SequenceFrame<'_> {
        let buffer = run(self.show, &mut self.workspace, time);
        SequenceFrame {
            show: self.show,
            colors: self.workspace.buffers[buffer],
            outputs: self.workspace.outputs,
        }
    }

    /// The frame at `time`, borrowing the buffer rather than this playback.
    pub fn frame(mut self, time: SampleTime) -> SequenceFrame<'w>
    where
        'a: 'w,
    {
        let buffer = run(self.show, &mut self.workspace, time);
        let Workspace {
            buffers, outputs, ..
        } = self.workspace;
        let Some(colors) = buffers.into_iter().nth(buffer) else {
            unreachable!("evaluation names one of the buffers")
        };
        SequenceFrame {
            show: self.show,
            colors,
            outputs,
        }
    }

    /// The output bytes of the latest frame.
    pub fn outputs(self) -> impl ExactSizeIterator<Item = OutputFrame<'w>>
    where
        'a: 'w,
    {
        output_frames(self.show, self.workspace.outputs)
    }

    /// Effects active at `time` as commands have placed them.
    pub fn active_effect_count(&self, time: SampleTime) -> usize {
        let rig = &self.workspace.rig;
        let timeline = Timeline::new(self.show, rig.timing, rig.layer_order, rig.layer_ends);
        (0..self.show.effect_count())
            .filter(|&effect| timeline.timing(effect).is_active(time))
            .count()
    }

    /// Apply a command at its time. Commands arrive in time order; one the
    /// show does not address is ignored.
    pub fn apply(&mut self, event: RigEvent) {
        apply(self.show, &mut self.workspace, event);
    }
}

/// The frame of `time` at the show's frame rate: the nearest, as frame times
/// can land microseconds off a frame's start.
fn frame_at(time: SampleTime, frame_rate: u32) -> u32 {
    let Some((frame, rest)) = mul_div(time.as_ticks(), frame_rate, MICROS_PER_SECOND) else {
        unreachable!("frame rates are below a million")
    };
    frame + u32::from(rest >= MICROS_PER_SECOND - rest)
}

/// Evaluate the frame of `time` in `workspace` unless its buffers still hold
/// it: its colors, then its output bytes. Returns the buffer holding the
/// colors. A frame after the one before advances its state by the frames
/// between them, the same frame after a command by none, and any other is a
/// seek: one frame on from the state it has.
pub(crate) fn run(show: Show<'_>, workspace: &mut Workspace<'_>, time: SampleTime) -> usize {
    let frame = frame_at(time, show.frame_rate());
    let [last, held] = *workspace.frame;
    if last == frame && held != 0 {
        return output_buffer(show);
    }
    let frames_elapsed = if last == NONE || frame < last {
        1
    } else {
        frame - last
    };
    *workspace.frame = [frame, 1];
    let buffer = evaluate_frame(show, workspace, time, frames_elapsed);
    crate::patch::evaluate(show, workspace.buffers[buffer], workspace.outputs);
    buffer
}

/// Borrowed logical colors and packed controller outputs from one evaluation.
#[derive(Clone, Copy)]
pub struct SequenceFrame<'a> {
    pub(crate) show: Show<'a>,
    pub(crate) colors: &'a [Color],
    pub(crate) outputs: &'a [u8],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FixtureFrame<'a> {
    pub fixture_id: u32,
    pub pixels: &'a [Color],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OutputFrame<'a> {
    pub output: &'a image::Output,
    pub bytes: &'a [u8],
}

impl<'a> SequenceFrame<'a> {
    pub fn colors(&self) -> &'a [Color] {
        self.colors
    }

    pub fn fixtures(&self) -> impl ExactSizeIterator<Item = FixtureFrame<'a>> + 'a {
        let colors = self.colors;
        self.show.fixtures().iter().map(move |fixture| {
            let start = fixture.offset as usize;
            FixtureFrame {
                fixture_id: fixture.id,
                pixels: &colors[start..start + fixture.pixel_count as usize],
            }
        })
    }

    pub fn outputs(&self) -> impl ExactSizeIterator<Item = OutputFrame<'a>> + 'a {
        output_frames(self.show, self.outputs)
    }
}

pub(crate) fn output_frames<'a>(
    show: Show<'a>,
    bytes: &'a [u8],
) -> impl ExactSizeIterator<Item = OutputFrame<'a>> + 'a {
    show.outputs().iter().map(move |output| {
        let start = output.offset as usize;
        OutputFrame {
            output,
            bytes: &bytes[start..start + output.width as usize],
        }
    })
}

impl Show<'_> {
    pub fn voice_count(&self) -> usize {
        self.s.voices.len()
    }

    pub fn collection_count(&self) -> usize {
        self.s.collections.len()
    }

    pub fn control_count(&self) -> usize {
        self.s.controls.len()
    }

    /// Whether `command` addresses a voice, collection or control of this show.
    pub fn accepts(&self, command: RigCommand) -> bool {
        match command {
            RigCommand::Start { voice } | RigCommand::Stop { voice } => {
                usize::from(voice) < self.voice_count()
            }
            RigCommand::Mark { collection } => usize::from(collection) < self.collection_count(),
            RigCommand::Control { control, value } => {
                usize::from(control) < self.control_count() && (0.0..=1.0).contains(&value)
            }
        }
    }
}

pub(crate) fn apply(show: Show<'_>, workspace: &mut Workspace<'_>, event: RigEvent) {
    if !show.accepts(event.command) {
        return;
    }
    let s = &show.s;
    let time = event.time;
    let ticks = time.as_ticks();
    let rig = &mut workspace.rig;
    match event.command {
        RigCommand::Start { voice } => {
            let voice = &s.voices[usize::from(voice)];
            rig.timing[voice.effect as usize] = Timing {
                start: ticks,
                duration: if voice.duration == NONE {
                    show.duration().as_ticks().saturating_sub(ticks)
                } else {
                    voice.duration
                },
            };
            if let Some(layer) = s
                .layers
                .iter()
                .find(|layer| layer.effects.get(s.layer_order).contains(&voice.effect))
            {
                index_layer(layer, rig.timing, rig.layer_order, rig.layer_ends);
            }
        }
        RigCommand::Stop { voice } => {
            let effect = s.voices[usize::from(voice)].effect as usize;
            let timing = &mut rig.timing[effect];
            if timing.is_active(time) {
                timing.duration = ticks - timing.start;
            }
        }
        RigCommand::Mark { collection } => {
            let track = &mut rig.live_tracks[usize::from(collection)];
            let start = track.start as usize;
            let marks = &mut rig.live_ticks[start..start + LIVE_MARK_CAPACITY];
            if track.len as usize == LIVE_MARK_CAPACITY {
                marks.copy_within(1.., 0);
                track.len -= 1;
            }
            marks[track.len as usize] = ticks;
            track.len += 1;
        }
        RigCommand::Control { control, value } => {
            for target in s.controls[usize::from(control)]
                .targets
                .get(s.control_targets)
            {
                let effect = target.effect as usize;
                let Some(plan) = show.plan(s.effects[effect].automation) else {
                    continue;
                };
                let start = plan.bindings.start as usize;
                let overrides = &mut rig.overrides[start..start + plan.bindings.len as usize];
                let origin = rig.timing[effect].start;
                set_value(
                    show,
                    plan,
                    overrides,
                    target.binding as usize,
                    value,
                    time,
                    origin,
                );
            }
        }
    }
    workspace.frame[1] = 0;
}
