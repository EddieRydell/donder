pub(crate) use crate::identity::InputDeviceId;
use donder_runtime_types::Identifier;

/// A control of an input device, numbered from 1 in authored order.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct InputControlId(pub u32);

/// Hardware a live performance plays from: its controls, each bound to one
/// input of its driver.
#[derive(Clone, Debug, PartialEq)]
pub struct InputDevice {
    pub id: InputDeviceId,
    pub description: Option<String>,
    pub driver: InputDriver,
    pub controls: Vec<InputControl>,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum InputDriver {
    Keyboard,
    /// MIDI input ports: those whose name contains `port`, or every port.
    Midi {
        port: Option<String>,
    },
    /// OSC messages sent to this UDP port, on every network interface.
    Osc {
        port: u16,
    },
}

impl InputDriver {
    /// Whether this driver reads `input`.
    fn reads(&self, input: &ControlInput) -> bool {
        match self {
            Self::Keyboard => matches!(input, ControlInput::Key(_)),
            Self::Midi { .. } => matches!(
                input,
                ControlInput::Note(_) | ControlInput::ControlChange(_) | ControlInput::PitchBend
            ),
            Self::Osc { .. } => matches!(input, ControlInput::Address(_)),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct InputControl {
    pub id: InputControlId,
    /// Unique within the device; rigs refer to it.
    pub name: Identifier,
    pub description: Option<String>,
    pub kind: InputControlKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum InputControlKind {
    /// Pressed or released.
    Button(ButtonInput),
    /// A position from 0 to 1.
    Dial(DialInput),
    /// A range of MIDI notes, each its own press: a piano's keys.
    Keys(KeyRange),
}

/// MIDI notes from `low` through `high`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct KeyRange {
    pub low: u8,
    pub high: u8,
}

impl KeyRange {
    pub fn contains(self, note: u8) -> bool {
        (self.low..=self.high).contains(&note)
    }
}

/// The highest MIDI note number.
pub const MAX_MIDI_NOTE: u8 = 127;
/// The highest MIDI control change number; 120 and up are channel modes.
pub const MAX_MIDI_CONTROL_CHANGE: u8 = 119;

/// An OSC address a control reads, such as `/chord/change`: a `/` and then
/// address characters, matched exactly.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct OscAddress(pub String);

impl OscAddress {
    /// Characters OSC reserves for address patterns and separators.
    const RESERVED: &'static [char] = &['*', '?', '[', ']', '{', '}', '#', ','];

    /// Whether it starts with `/` and has only printable ASCII characters
    /// that are not spaces or OSC's pattern characters.
    fn is_valid(&self) -> bool {
        self.0.starts_with('/')
            && self
                .0
                .chars()
                .all(|c| c.is_ascii_graphic() && !Self::RESERVED.contains(&c))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum ButtonInput {
    Key(KeyCode),
    /// A MIDI note, held from note on to note off.
    Note(u8),
    /// A MIDI control change, held at 64 and above, as a sustain pedal is.
    ControlChange(u8),
    /// An OSC address: pressed by a first argument above 0, released by 0,
    /// and pressed and released at once by a message with no arguments.
    Address(OscAddress),
}

#[derive(Clone, Debug, PartialEq)]
pub enum DialInput {
    /// A MIDI control change, from 0 to 119.
    ControlChange(u8),
    /// The MIDI pitch bend wheel, from fully down to fully up.
    PitchBend,
    /// An OSC address whose first argument runs from `low` to `high`.
    Address {
        path: OscAddress,
        low: f32,
        high: f32,
    },
}

/// What a control reads, whichever kind of control reads it. No two controls
/// of a device read the same input.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum ControlInput {
    Key(KeyCode),
    Note(u8),
    ControlChange(u8),
    PitchBend,
    Address(OscAddress),
}

impl InputControlKind {
    /// Every input the control reads: one, or each key of its range.
    fn sources(&self) -> Vec<ControlInput> {
        match self {
            Self::Button(ButtonInput::Key(code)) => vec![ControlInput::Key(*code)],
            Self::Button(ButtonInput::Note(note)) => vec![ControlInput::Note(*note)],
            Self::Button(ButtonInput::ControlChange(number))
            | Self::Dial(DialInput::ControlChange(number)) => {
                vec![ControlInput::ControlChange(*number)]
            }
            Self::Dial(DialInput::PitchBend) => vec![ControlInput::PitchBend],
            Self::Button(ButtonInput::Address(path))
            | Self::Dial(DialInput::Address { path, .. }) => {
                vec![ControlInput::Address(path.clone())]
            }
            Self::Keys(range) => (range.low..=range.high).map(ControlInput::Note).collect(),
        }
    }

    /// Whether it is a single button, which switches and `Control` mappings
    /// can play from.
    pub fn is_button(&self) -> bool {
        matches!(self, Self::Button(_))
    }

    /// Whether it is pressed and released, as a button or the keys of a
    /// range are, so it can trigger effects and add marks.
    pub fn is_pressed(&self) -> bool {
        matches!(self, Self::Button(_) | Self::Keys(_))
    }

    /// The notes a press can carry: a note button's, or its range.
    pub fn notes(&self) -> Option<KeyRange> {
        match self {
            Self::Button(ButtonInput::Note(note)) => Some(KeyRange {
                low: *note,
                high: *note,
            }),
            Self::Keys(range) => Some(*range),
            Self::Button(
                ButtonInput::Key(_) | ButtonInput::ControlChange(_) | ButtonInput::Address(_),
            )
            | Self::Dial(_) => None,
        }
    }

    /// Whether a dial's range runs between two finite values, from lower to
    /// higher; other controls have no range.
    fn range_is_valid(&self) -> bool {
        match self {
            Self::Dial(DialInput::Address { low, high, .. }) => {
                low.is_finite() && high.is_finite() && low < high
            }
            Self::Button(_) | Self::Dial(_) | Self::Keys(_) => true,
        }
    }
}

impl ControlInput {
    /// Whether the number it reads is one MIDI has, leaving out the control
    /// changes 120 and up, which are channel modes.
    fn in_range(&self) -> bool {
        match self {
            Self::Key(_) | Self::PitchBend => true,
            Self::Note(note) => *note <= MAX_MIDI_NOTE,
            Self::ControlChange(number) => *number <= MAX_MIDI_CONTROL_CHANGE,
            Self::Address(_) => true,
        }
    }
}

impl std::fmt::Display for ControlInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Key(code) => write!(f, "`{}`", code.name()),
            Self::Note(note) => write!(f, "note {note}"),
            Self::ControlChange(number) => write!(f, "control change {number}"),
            Self::PitchBend => f.write_str("pitch bend"),
            Self::Address(address) => write!(f, "OSC address `{}`", address.0),
        }
    }
}

/// A control of an input device: a button, a dial or a range of keys.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ControlRef {
    pub device: InputDeviceId,
    pub control: InputControlId,
}

macro_rules! key_codes {
    ($($name:ident),* $(,)?) => {
        /// A physical keyboard key, by the browser `KeyboardEvent.code` name,
        /// which winit's `KeyCode` also uses.
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
        pub enum KeyCode {
            $($name),*
        }

        impl KeyCode {
            /// Every key, in the order the language lists them.
            pub const ALL: &'static [KeyCode] = &[$(KeyCode::$name),*];

            /// The key's `KeyboardEvent.code` name.
            pub fn name(self) -> &'static str {
                match self {
                    $(KeyCode::$name => stringify!($name)),*
                }
            }

            pub fn from_name(name: &str) -> Option<Self> {
                match name {
                    $(stringify!($name) => Some(KeyCode::$name),)*
                    _ => None,
                }
            }
        }
    };
}

key_codes!(
    KeyA,
    KeyB,
    KeyC,
    KeyD,
    KeyE,
    KeyF,
    KeyG,
    KeyH,
    KeyI,
    KeyJ,
    KeyK,
    KeyL,
    KeyM,
    KeyN,
    KeyO,
    KeyP,
    KeyQ,
    KeyR,
    KeyS,
    KeyT,
    KeyU,
    KeyV,
    KeyW,
    KeyX,
    KeyY,
    KeyZ,
    Digit0,
    Digit1,
    Digit2,
    Digit3,
    Digit4,
    Digit5,
    Digit6,
    Digit7,
    Digit8,
    Digit9,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    Space,
    Enter,
    Tab,
    Escape,
    Backspace,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    ShiftLeft,
    ShiftRight,
    ControlLeft,
    ControlRight,
    AltLeft,
    AltRight,
    Minus,
    Equal,
    BracketLeft,
    BracketRight,
    Backslash,
    Semicolon,
    Quote,
    Comma,
    Period,
    Slash,
    Backquote,
    Numpad0,
    Numpad1,
    Numpad2,
    Numpad3,
    Numpad4,
    Numpad5,
    Numpad6,
    Numpad7,
    Numpad8,
    Numpad9,
    NumpadAdd,
    NumpadSubtract,
    NumpadMultiply,
    NumpadDivide,
    NumpadDecimal,
    NumpadEnter,
    Insert,
    Delete,
    Home,
    End,
    PageUp,
    PageDown,
    CapsLock,
);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InputDeviceValidationError {
    DuplicateControl(InputControlId),
    /// Control names are `snake_case`.
    InvalidControlName(InputControlId, Identifier),
    /// Control names are unique within their device.
    DuplicateControlName(InputControlId, Identifier),
    /// Two controls read the same input.
    DuplicateInput {
        control: InputControlId,
        name: Identifier,
        source: ControlInput,
        other: Identifier,
    },
    /// A control reads an input its device's driver does not have.
    ForeignInput(InputControlId, Identifier),
    /// A note or control change number MIDI does not have, or keys whose
    /// lowest note is above their highest.
    InputOutOfRange(InputControlId, Identifier),
    /// An OSC address that is not `/` followed by address characters.
    InvalidAddress(InputControlId, Identifier),
    /// A dial whose range is not two finite values from lower to higher.
    InvalidDialRange(InputControlId, Identifier),
    /// An OSC device listening on port 0.
    InvalidOscPort,
}

impl InputDeviceValidationError {
    /// The control the error concerns.
    pub(crate) fn control(&self) -> Option<InputControlId> {
        match self {
            Self::DuplicateControl(control)
            | Self::InvalidControlName(control, _)
            | Self::DuplicateControlName(control, _)
            | Self::DuplicateInput { control, .. }
            | Self::ForeignInput(control, _)
            | Self::InputOutOfRange(control, _)
            | Self::InvalidAddress(control, _)
            | Self::InvalidDialRange(control, _) => Some(*control),
            Self::InvalidOscPort => None,
        }
    }
}

impl std::fmt::Display for InputDeviceValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateControl(id) => write!(f, "control {} appears twice", id.0),
            Self::InvalidControlName(_, name) => {
                write!(f, "`{}` is not a snake_case name", name.as_str())
            }
            Self::DuplicateControlName(_, name) => {
                write!(f, "control `{}` is declared twice", name.as_str())
            }
            Self::DuplicateInput {
                name,
                source,
                other,
                ..
            } => write!(
                f,
                "control `{}` reads {source}, which control `{}` already reads",
                name.as_str(),
                other.as_str()
            ),
            Self::ForeignInput(_, name) => write!(
                f,
                "control `{}` reads an input its device's driver does not have",
                name.as_str()
            ),
            Self::InputOutOfRange(_, name) => write!(
                f,
                "control `{}` reads a note above {MAX_MIDI_NOTE}, a control change above {MAX_MIDI_CONTROL_CHANGE}, or keys from a higher note to a lower one",
                name.as_str()
            ),
            Self::InvalidAddress(_, name) => write!(
                f,
                "control `{}` reads an OSC address that is not `/` followed by printable characters other than spaces and `*?[]{{}}#,`",
                name.as_str()
            ),
            Self::InvalidDialRange(_, name) => write!(
                f,
                "dial `{}` must run from a lower to a higher finite value",
                name.as_str()
            ),
            Self::InvalidOscPort => f.write_str("an OSC device listens on a port from 1 to 65535"),
        }
    }
}

impl InputDevice {
    pub fn validate(&self) -> Result<(), InputDeviceValidationError> {
        use InputDeviceValidationError as Error;
        let mut ids = std::collections::HashSet::new();
        let mut names = std::collections::HashSet::new();
        let mut inputs = std::collections::HashMap::new();
        if self.driver == (InputDriver::Osc { port: 0 }) {
            return Err(Error::InvalidOscPort);
        }
        for control in &self.controls {
            let (id, name) = (control.id, || control.name.clone());
            if !ids.insert(control.id) {
                return Err(Error::DuplicateControl(id));
            }
            if !donder_language::NameKind::Object.accepts(control.name.as_str()) {
                return Err(Error::InvalidControlName(id, name()));
            }
            if !names.insert(&control.name) {
                return Err(Error::DuplicateControlName(id, name()));
            }
            let sources = control.kind.sources();
            if sources.iter().any(|source| !self.driver.reads(source)) {
                return Err(Error::ForeignInput(id, name()));
            }
            if sources.iter().any(
                |source| matches!(source, ControlInput::Address(address) if !address.is_valid()),
            ) {
                return Err(Error::InvalidAddress(id, name()));
            }
            if !control.kind.range_is_valid() {
                return Err(Error::InvalidDialRange(id, name()));
            }
            if sources.is_empty() || sources.iter().any(|source| !source.in_range()) {
                return Err(Error::InputOutOfRange(id, name()));
            }
            for source in sources {
                if let Some(other) = inputs.insert(source.clone(), &control.name) {
                    return Err(Error::DuplicateInput {
                        control: id,
                        name: name(),
                        source,
                        other: other.clone(),
                    });
                }
            }
        }
        Ok(())
    }

    pub fn control(&self, id: InputControlId) -> Option<&InputControl> {
        self.controls.iter().find(|control| control.id == id)
    }
}

impl crate::ownership::Identified for InputDevice {
    type Id = InputDeviceId;
    fn id(&self) -> &Self::Id {
        &self.id
    }
}

pub type OwnedInputDevice = crate::ownership::OwnedMember<InputDevice, InputDeviceId>;
