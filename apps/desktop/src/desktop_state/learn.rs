//! Learning an input: the next note, knob or OSC message a device sends becomes
//! what a control reads, so a control is bound by using it. A performance that
//! already reads the device supplies its messages; otherwise learning opens the
//! device itself. Keyboard keys are learned by the window.

use std::any::Any;
use std::sync::mpsc;

use super::perform::reads_port;
use super::{DesktopState, lock_unpoisoned};
use crate::midi::{ChannelMessage, MidiInputs, MidiMessage};
use crate::osc::{OscInputs, OscMessage};
use donder_editor::dto::{GuiControlInput, GuiInputControlKind, GuiInputDriver, LearnedInput};
use donder_model::MAX_MIDI_CONTROL_CHANGE;

/// What a session listens to.
enum Source {
    Midi { port: Option<String> },
    Osc { port: u16 },
}

/// What keeps a session's listeners open until it ends.
type Listeners = Box<dyn Any + Send>;

struct Session {
    /// Which request the session answers, so a late cancel cannot end a newer one.
    token: u32,
    source: Source,
    learned: mpsc::Sender<Option<LearnedInput>>,
    /// The listeners the session opened itself. A device a performance already
    /// reads is not opened twice: the performance hands its messages on instead.
    _listeners: Option<Listeners>,
}

/// The one input being learned, if any.
#[derive(Default)]
pub(super) struct LearnService {
    session: Option<Session>,
}

impl DesktopState {
    /// Listen to a device until it sends something a control can read. The
    /// receiver gets that, or `None` when learning is cancelled or replaced.
    pub(crate) fn begin_learning(
        &self,
        token: u32,
        driver: &GuiInputDriver,
    ) -> Result<mpsc::Receiver<Option<LearnedInput>>, String> {
        let (source, listeners) = match driver {
            GuiInputDriver::Keyboard => return Err("The window learns keyboard keys.".into()),
            GuiInputDriver::Midi { port } => {
                let listeners = (!self.performance_reads_midi(port.as_deref())).then(|| {
                    let wanted = port.clone();
                    let state = self.weak.clone();
                    Box::new(MidiInputs::start(
                        move |name| reads_port(wanted.as_deref(), name),
                        move |name, message| {
                            if let Some(state) = state.upgrade() {
                                DesktopState(state).offer_midi(name, message);
                            }
                        },
                        |_| {},
                    )) as Listeners
                });
                (Source::Midi { port: port.clone() }, listeners)
            }
            GuiInputDriver::Osc { port } => {
                let listeners = (!self.performance_reads_osc(*port)).then(|| {
                    let state = self.weak.clone();
                    Box::new(OscInputs::start(vec![*port], move |port, message| {
                        if let Some(state) = state.upgrade() {
                            DesktopState(state).offer_osc(port, &message);
                        }
                    })) as Listeners
                });
                (Source::Osc { port: *port }, listeners)
            }
        };
        let (learned, receiver) = mpsc::channel();
        let session = Session {
            token,
            source,
            learned,
            _listeners: listeners,
        };
        if let Some(previous) = lock_unpoisoned(&self.learn).session.replace(session) {
            let _ = previous.learned.send(None);
        }
        Ok(receiver)
    }

    /// Stop learning, or only the request `token` made; the caller waiting for
    /// an input gets `None`.
    pub(crate) fn end_learning(&self, token: Option<u32>) {
        let mut service = lock_unpoisoned(&self.learn);
        if service
            .session
            .as_ref()
            .is_some_and(|session| token.is_none_or(|token| session.token == token))
            && let Some(session) = service.session.take()
        {
            let _ = session.learned.send(None);
        }
    }

    /// A MIDI message from `port`: the input being learned, when it reads that
    /// port and the message is a note on, a control change or pitch bend.
    pub(super) fn offer_midi(&self, port: &str, message: ChannelMessage) {
        let channel = Some(message.channel.0);
        let learned = match message.message {
            MidiMessage::NoteOn { note, .. } => LearnedInput {
                kind: GuiInputControlKind::Button,
                input: GuiControlInput::Notes {
                    low: note,
                    high: note,
                    channel,
                },
            },
            // Numbers above the last control change are channel mode messages.
            MidiMessage::ControlChange { number, .. } if number <= MAX_MIDI_CONTROL_CHANGE => {
                LearnedInput {
                    kind: GuiInputControlKind::Axis,
                    input: GuiControlInput::ControlChange { number, channel },
                }
            }
            MidiMessage::PitchBend { .. } => LearnedInput {
                kind: GuiInputControlKind::Axis,
                input: GuiControlInput::PitchBend { channel },
            },
            MidiMessage::NoteOff { .. } | MidiMessage::ControlChange { .. } => return,
        };
        self.finish_learning(
            |source| matches!(source, Source::Midi { port: wanted } if reads_port(wanted.as_deref(), port)),
            learned,
        );
    }

    /// An OSC message to `port`: the input being learned, when it listens on
    /// that port. A message with no value, or a value of 0 or 1, suits a button;
    /// any other value suits an axis.
    pub(super) fn offer_osc(&self, port: u16, message: &OscMessage) {
        let path = message.address.clone();
        let learned = if message
            .value
            .is_none_or(|value| value == 0.0 || value == 1.0)
        {
            LearnedInput {
                kind: GuiInputControlKind::Button,
                input: GuiControlInput::Address { path },
            }
        } else {
            LearnedInput {
                kind: GuiInputControlKind::Axis,
                input: GuiControlInput::AddressRange {
                    path,
                    low: 0.0,
                    high: 1.0,
                },
            }
        };
        self.finish_learning(
            |source| matches!(source, Source::Osc { port: wanted } if *wanted == port),
            learned,
        );
    }

    /// Hand `learned` to the session when `source` is what it listens to.
    fn finish_learning(&self, source: impl FnOnce(&Source) -> bool, learned: LearnedInput) {
        let mut service = lock_unpoisoned(&self.learn);
        if service
            .session
            .as_ref()
            .is_some_and(|session| source(&session.source))
            && let Some(session) = service.session.take()
        {
            let _ = session.learned.send(Some(learned));
        }
    }
}
