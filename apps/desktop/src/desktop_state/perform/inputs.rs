//! A performance's inputs on its console's controls. Every evaluator routes
//! them itself; the host keeps only which presses are held, so it shows them
//! and sends a press once however often a key repeats.

use donder_model::ControlRef;
use donder_runtime_types::{ConsoleEvent, InputEvent, InputHappening, SampleTime};

pub(crate) struct ConsoleInputs {
    /// The console's controls, by the index inputs name them by.
    controls: Box<[ControlRef]>,
    /// The presses held down: a control, and its note when it reads notes.
    held: Vec<(u16, Option<u8>)>,
}

impl ConsoleInputs {
    pub(crate) fn new(controls: Vec<ControlRef>) -> Self {
        Self {
            controls: controls.into(),
            held: Vec::new(),
        }
    }

    pub(crate) fn controls(&self) -> &[ControlRef] {
        &self.controls
    }

    /// Whether `input` is held down, or any of its notes.
    pub(crate) fn pressed(&self, input: usize) -> bool {
        self.held
            .iter()
            .any(|&(control, _)| usize::from(control) == input)
    }

    /// A press of `button` at `time`: a computer key or a pedal, at full
    /// velocity. A press of a button already down is a key repeat.
    pub(crate) fn press(&mut self, button: usize, time: SampleTime) -> Vec<ConsoleEvent> {
        self.touch(button, None, InputHappening::Press, 1.0, time)
    }

    /// Striking `note` of the note button `input` with `velocity` in `0..=1`
    /// at `time`; each note is its own press.
    pub(crate) fn strike(
        &mut self,
        input: usize,
        note: u8,
        velocity: f32,
        time: SampleTime,
    ) -> Vec<ConsoleEvent> {
        self.touch(input, Some(note), InputHappening::Press, velocity, time)
    }

    pub(crate) fn release(&mut self, button: usize, time: SampleTime) -> Vec<ConsoleEvent> {
        self.touch(button, None, InputHappening::Release, 0.0, time)
    }

    pub(crate) fn release_note(
        &mut self,
        input: usize,
        note: u8,
        time: SampleTime,
    ) -> Vec<ConsoleEvent> {
        self.touch(input, Some(note), InputHappening::Release, 0.0, time)
    }

    /// A move of the axis `input` to `value`, in `0..=1`, at `time`.
    pub(crate) fn move_axis(
        &mut self,
        input: usize,
        value: f32,
        time: SampleTime,
    ) -> Vec<ConsoleEvent> {
        self.touch(input, None, InputHappening::Move, value, time)
    }

    /// Release every held button and note, as when the performer's window
    /// loses focus.
    pub(crate) fn release_all(&mut self, time: SampleTime) -> Vec<ConsoleEvent> {
        std::mem::take(&mut self.held)
            .into_iter()
            .map(|(control, note)| event(control, note, InputHappening::Release, 0.0, time))
            .collect()
    }

    /// The inputs `events`, which name `controls`, on these controls: an
    /// input of a control these do not read is left out. These hold what the
    /// inputs left held.
    pub(crate) fn resume(
        &mut self,
        controls: &[ControlRef],
        events: &[ConsoleEvent],
    ) -> Vec<ConsoleEvent> {
        events
            .iter()
            .filter_map(|event| {
                let input = event.input;
                let control = controls.get(usize::from(input.control))?;
                let index = self.controls.iter().position(|read| read == control)?;
                let resumed =
                    self.touch(index, input.note, input.happening, input.value, event.time);
                resumed.into_iter().next()
            })
            .collect()
    }

    fn touch(
        &mut self,
        input: usize,
        note: Option<u8>,
        happening: InputHappening,
        value: f32,
        time: SampleTime,
    ) -> Vec<ConsoleEvent> {
        let control = input as u16;
        let press = (control, note);
        match happening {
            InputHappening::Press if self.held.contains(&press) => return Vec::new(),
            InputHappening::Press => self.held.push(press),
            InputHappening::Release => {
                let Some(held) = self.held.iter().position(|held| *held == press) else {
                    return Vec::new();
                };
                self.held.swap_remove(held);
            }
            InputHappening::Move => {}
        }
        vec![event(control, note, happening, value.clamp(0.0, 1.0), time)]
    }
}

fn event(
    control: u16,
    note: Option<u8>,
    happening: InputHappening,
    value: f32,
    time: SampleTime,
) -> ConsoleEvent {
    ConsoleEvent {
        time,
        input: InputEvent {
            control,
            note,
            happening,
            value,
        },
    }
}
