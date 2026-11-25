//! Playback of a show in a caller's buffer: frames, and the console commands
//! that start and stop voices, add live marks and set live automations.
//!
//! Commands change only the workspace: a voice's start and duration, its
//! layer's start order, live marks, and the live store.
use crate::format;
use donder_runtime_types::{Color, MICROS_PER_SECOND, SampleDuration, SampleTime, mul_div};

use crate::evaluation::{evaluate_frame, index_layer, root_buffer};
use crate::view::Show;
use crate::workspace::{LIVE_MARK_CAPACITY, Layout, Timing, Workspace, carve};

/// What a routed input does to a console's playback, by index of the
/// prepared console's voices, live mark collections and live automations.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ConsoleCommand {
    /// Start a voice from its beginning for `duration`, restarting it if it
    /// is playing.
    Start {
        voice: u16,
        duration: SampleDuration,
    },
    /// End a voice; a voice that is not playing is unchanged.
    Stop { voice: u16 },
    /// Add a mark to a live mark collection.
    Mark { collection: u16 },
    /// Set a live automation to a value in `0..=1`.
    Set { automation: u16, value: f32 },
}

/// A show playing in a caller's buffer of at least [`Layout::words`] words.
pub(crate) struct Playback<'a, 'w> {
    show: Show<'a>,
    workspace: Workspace<'w>,
}

impl<'a, 'w> Playback<'a, 'w> {
    /// Start `show` in `buffer`.
    pub(crate) fn new(show: Show<'a>, layout: &Layout, buffer: &'w mut [u32]) -> Self {
        Self {
            show,
            workspace: carve(show, layout, bytemuck::cast_slice_mut(buffer), true),
        }
    }

    /// Continue `show` in the `buffer` an earlier playback of it left, as a
    /// caller that keeps the buffer between frames does.
    pub(crate) fn resume(show: Show<'a>, layout: &Layout, buffer: &'w mut [u32]) -> Self {
        Self {
            show,
            workspace: carve(show, layout, bytemuck::cast_slice_mut(buffer), false),
        }
    }

    /// The frame at `time`, borrowing the buffer rather than this playback.
    pub(crate) fn frame(mut self, time: SampleTime) -> SequenceFrame<'w>
    where
        'a: 'w,
    {
        let buffer = run(self.show, &mut self.workspace, time);
        let Workspace {
            buffers,
            outputs,
            pixels,
            ..
        } = self.workspace;
        let colors = &mut buffers[buffer * pixels..(buffer + 1) * pixels];
        SequenceFrame {
            show: self.show,
            colors,
            outputs,
        }
    }

    /// Apply a command at its time. Commands arrive in time order; one the
    /// show does not address is ignored.
    pub(crate) fn apply(&mut self, time: SampleTime, command: ConsoleCommand) {
        apply(self.show, &mut self.workspace, time, command);
    }
}

/// The frame `ticks` after a start at `frame_rate`: the nearest, as frame
/// times can land microseconds off a frame's start.
pub(crate) fn frame_at(ticks: u32, frame_rate: u32) -> u32 {
    let Some((frame, rest)) = mul_div(ticks, frame_rate, MICROS_PER_SECOND) else {
        unreachable!("frame rates are below a million")
    };
    frame + u32::from(rest >= MICROS_PER_SECOND - rest)
}

/// Evaluate the frame of `time` in `workspace` unless its buffers still hold
/// it: its colors, then its output bytes. Returns the buffer holding the
/// colors.
fn run(show: Show<'_>, workspace: &mut Workspace<'_>, time: SampleTime) -> usize {
    let frame = frame_at(time.as_ticks(), show.frame_rate());
    let [last, held] = *workspace.frame;
    if last == frame && held != 0 {
        return root_buffer(show);
    }
    *workspace.frame = [frame, 1];
    let buffer = evaluate_frame(show, workspace, time);
    let colors = &workspace.buffers[buffer * workspace.pixels..(buffer + 1) * workspace.pixels];
    crate::patch::evaluate(show, colors, workspace.outputs, frame);
    buffer
}

/// Borrowed logical colors and packed controller outputs from one evaluation.
#[derive(Clone, Copy)]
pub struct SequenceFrame<'a> {
    show: Show<'a>,
    colors: &'a [Color],
    outputs: &'a [u8],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FixtureFrame<'a> {
    pub fixture_id: u32,
    pub pixels: &'a [Color],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OutputFrame<'a> {
    pub output: &'a format::Output,
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

fn output_frames<'a>(
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

fn apply(show: Show<'_>, workspace: &mut Workspace<'_>, time: SampleTime, command: ConsoleCommand) {
    let s = &show.s;
    let ticks = time.as_ticks();
    let console = &mut workspace.console;
    match command {
        ConsoleCommand::Start { voice, duration } => {
            let clip = s.voices[usize::from(voice)];
            console.timing[clip as usize] = Timing {
                start: ticks,
                duration: duration.as_ticks(),
            };
            // Its integrals of live values count from its start.
            for snapshot in s.snapshots.iter().filter(|snapshot| snapshot.clip == clip) {
                console.live[snapshot.word as usize] =
                    live_integral(show, console.live, snapshot.automation, ticks);
            }
            if let Some(layer) = s
                .layers
                .iter()
                .find(|layer| layer.clips.get(s.layer_order).contains(&clip))
            {
                index_layer(
                    layer,
                    console.timing,
                    console.layer_order,
                    console.layer_ends,
                );
            }
        }
        ConsoleCommand::Stop { voice } => {
            let clip = s.voices[usize::from(voice)] as usize;
            let timing = &mut console.timing[clip];
            if timing.is_active(time) {
                timing.duration = ticks - timing.start;
            }
        }
        ConsoleCommand::Mark { collection } => {
            let track = &mut console.live_tracks[usize::from(collection)];
            let start = track.start as usize;
            let marks = &mut console.live_ticks[start..start + LIVE_MARK_CAPACITY];
            if track.len as usize == LIVE_MARK_CAPACITY {
                marks.copy_within(1.., 0);
                track.len -= 1;
            }
            marks[track.len as usize] = ticks;
            track.len += 1;
        }
        ConsoleCommand::Set { automation, value } => {
            let word = s.live_automations[usize::from(automation)].word as usize;
            let base = live_integral(show, console.live, u32::from(automation), ticks);
            console.live[word..word + 3].copy_from_slice(&[value, base, seconds(ticks)]);
            // Clips yet to start count their integrals from what the new
            // value makes theirs.
            for snapshot in s
                .snapshots
                .iter()
                .filter(|snapshot| snapshot.automation == u32::from(automation))
            {
                let start = console.timing[snapshot.clip as usize].start;
                if start > ticks {
                    console.live[snapshot.word as usize] =
                        live_integral(show, console.live, snapshot.automation, start);
                }
            }
        }
    }
    workspace.frame[1] = 0;
}

/// Seconds of show time `ticks`.
fn seconds(ticks: u32) -> f32 {
    donder_runtime_types::sample_duration_seconds_f32(SampleDuration::from_ticks(ticks))
}

/// Live automation `automation`'s integral over the session at `ticks`, as
/// its live store words hold it: `base + value * (t - since)`.
pub(crate) fn live_integral(show: Show<'_>, live: &[f32], automation: u32, ticks: u32) -> f32 {
    let word = show.s.live_automations[automation as usize].word as usize;
    let [value, base, since] = [live[word], live[word + 1], live[word + 2]];
    base + value * (seconds(ticks) - since)
}
