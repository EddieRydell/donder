//! Playback of an admitted show in a caller's buffer: frames, and the rig
//! commands that start and stop voices, add live marks and set controls.
//!
//! Commands change only the workspace: a voice's start and duration, its
//! layer's start order, live marks, and the parameter values automation
//! already writes each frame. Frame evaluation is unchanged.
use donder_runtime_types::image::{self, NONE};
use donder_runtime_types::{Color, RigCommand, RigEvent, SampleTime};

use crate::show::Show;
use crate::workspace::{DORMANT, LIVE_MARK_CAPACITY, Layout, Timing, Workspace, carve};

/// A show playing in a caller's buffer of [`Layout::words`] words.
pub struct Playback<'a, 'w> {
    show: Show<'a>,
    workspace: Workspace<'w>,
}

impl<'a, 'w> Playback<'a, 'w> {
    /// Start `show` in `buffer`; `None` when it is smaller than the show needs.
    pub fn new(show: Show<'a>, layout: &Layout, buffer: &'w mut [u64]) -> Option<Self> {
        Some(Self {
            show,
            workspace: carve(show, layout, bytemuck::cast_slice_mut(buffer), true)?,
        })
    }

    /// Continue `show` in the `buffer` an earlier playback of it left, as a
    /// caller that keeps the buffer between frames does.
    pub fn resume(show: Show<'a>, layout: &Layout, buffer: &'w mut [u64]) -> Option<Self> {
        Some(Self {
            show,
            workspace: carve(show, layout, bytemuck::cast_slice_mut(buffer), false)?,
        })
    }

    pub fn show(&self) -> Show<'a> {
        self.show
    }

    pub fn evaluate(&mut self, time: SampleTime) -> SequenceFrame<'_> {
        run(self.show, &mut self.workspace, time);
        SequenceFrame {
            show: self.show,
            colors: self.workspace.signal_buffer,
            outputs: self.workspace.outputs,
        }
    }

    /// The frame at `time`, borrowing the buffer rather than this playback.
    pub fn frame(mut self, time: SampleTime) -> SequenceFrame<'w>
    where
        'a: 'w,
    {
        run(self.show, &mut self.workspace, time);
        let Workspace {
            signal_buffer,
            outputs,
            ..
        } = self.workspace;
        SequenceFrame {
            show: self.show,
            colors: signal_buffer,
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
        let sampling = &self.workspace.sampling;
        let timeline = crate::evaluation::Timeline::new(
            self.show,
            sampling.timing,
            sampling.layer_order,
            sampling.layer_ends,
        );
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

/// Evaluate a frame in `workspace`: its colors, then its output bytes.
pub(crate) fn run(show: Show<'_>, workspace: &mut Workspace<'_>, time: SampleTime) {
    crate::evaluation::sample_signal_graph(show, time, workspace);
    crate::patch::evaluate(show, workspace.signal_buffer, workspace.outputs);
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
    let sampling = &mut workspace.sampling;
    match event.command {
        RigCommand::Start { voice } => {
            let voice = &s.voices[usize::from(voice)];
            let effect = voice.effect as usize;
            sampling.timing[effect] = Timing {
                start: ticks,
                duration: if voice.duration == NONE {
                    show.duration().as_ticks().saturating_sub(ticks)
                } else {
                    voice.duration
                },
            };
            if let Some(state) = state_of(show, effect) {
                sampling.effect_states.get(state).restart_at(ticks);
            }
            if let Some(layer) = s
                .layers
                .iter()
                .find(|layer| layer.effects.get(s.layer_order).contains(&voice.effect))
            {
                crate::evaluation::index_layer(
                    layer,
                    sampling.timing,
                    sampling.layer_order,
                    sampling.layer_ends,
                );
            }
            bind_marks(show, workspace, effect);
        }
        RigCommand::Stop { voice } => {
            let effect = s.voices[usize::from(voice)].effect as usize;
            let timing = &mut sampling.timing[effect];
            if timing.is_active(time) {
                timing.duration = ticks - timing.start;
            }
        }
        RigCommand::Mark { collection } => {
            let index = usize::from(collection);
            let track = &mut sampling.live_tracks[index];
            let ticks_buffer = &mut sampling.live_ticks
                [track.start as usize..track.start as usize + LIVE_MARK_CAPACITY];
            if track.len as usize == LIVE_MARK_CAPACITY {
                ticks_buffer.copy_within(1.., 0);
                track.len -= 1;
            }
            ticks_buffer[track.len as usize] = ticks;
            track.len += 1;
            for reader in s.collections[index].readers.get(s.readers) {
                bind_marks(show, workspace, reader.effect as usize);
            }
        }
        RigCommand::Control { control, value } => {
            for target in s.controls[usize::from(control)]
                .targets
                .get(s.control_targets)
            {
                if let Some(state) = state_of(show, target.effect as usize) {
                    sampling.effect_states.get(state).set_value(
                        show,
                        target.binding as usize,
                        value,
                        time,
                    );
                }
            }
        }
    }
    workspace.forget_samples();
}

fn state_of(show: Show<'_>, effect: usize) -> Option<usize> {
    let state = show.s.effects[effect].state;
    (state != NONE).then_some(state as usize)
}

/// Bind each live collection `effect` reads, measured from its start.
fn bind_marks(show: Show<'_>, workspace: &mut Workspace<'_>, effect: usize) {
    let s = &show.s;
    let sampling = &mut workspace.sampling;
    let origin = crate::evaluation::Timeline::new(
        show,
        sampling.timing,
        sampling.layer_order,
        sampling.layer_ends,
    )
    .timing(effect)
    .start;
    if origin == DORMANT {
        return;
    }
    let Some(state) = state_of(show, effect) else {
        return;
    };
    for (collection, record) in s.collections.iter().enumerate() {
        for reader in record.readers.get(s.readers) {
            if reader.effect as usize == effect {
                sampling.effect_states.get(state).bind_live(
                    reader.bank as usize,
                    collection as u32,
                    origin,
                );
            }
        }
    }
}
