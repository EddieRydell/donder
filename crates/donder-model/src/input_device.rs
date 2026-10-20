use crate::identity::ObjectIdentity;
use donder_runtime_types::Identifier;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct InputDeviceId(pub ObjectIdentity);

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
    pub fn reads(&self, input: &InputSource) -> bool {
        match self {
            Self::Keyboard => matches!(input, InputSource::Key(_)),
            Self::Midi { .. } => matches!(
                input,
                InputSource::Note(_) | InputSource::ControlChange(_) | InputSource::PitchBend
            ),
            Self::Osc { .. } => matches!(input, InputSource::Address(_)),
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
    pub fn is_valid(&self) -> bool {
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
pub enum InputSource {
    Key(KeyCode),
    Note(u8),
    ControlChange(u8),
    PitchBend,
    Address(OscAddress),
}

impl InputControlKind {
    /// Every input the control reads: one, or each key of its range.
    pub fn sources(&self) -> Vec<InputSource> {
        match self {
            Self::Button(ButtonInput::Key(code)) => vec![InputSource::Key(*code)],
            Self::Button(ButtonInput::Note(note)) => vec![InputSource::Note(*note)],
            Self::Button(ButtonInput::ControlChange(number))
            | Self::Dial(DialInput::ControlChange(number)) => {
                vec![InputSource::ControlChange(*number)]
            }
            Self::Dial(DialInput::PitchBend) => vec![InputSource::PitchBend],
            Self::Button(ButtonInput::Address(path))
            | Self::Dial(DialInput::Address { path, .. }) => {
                vec![InputSource::Address(path.clone())]
            }
            Self::Keys(range) => (range.low..=range.high).map(InputSource::Note).collect(),
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

impl InputSource {
    /// Whether the number it reads is one MIDI has, leaving out the control
    /// changes 120 and up, which are channel modes.
    pub fn in_range(&self) -> bool {
        match self {
            Self::Key(_) | Self::PitchBend => true,
            Self::Note(note) => *note <= MAX_MIDI_NOTE,
            Self::ControlChange(number) => *number <= MAX_MIDI_CONTROL_CHANGE,
            Self::Address(_) => true,
        }
    }
}

impl std::fmt::Display for InputSource {
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
    /// Control names are `snake_case` and unique within their device.
    InvalidControlName(Identifier),
    /// Two controls read the same input.
    DuplicateInput(Identifier),
    /// A control reads an input its device's driver does not have.
    ForeignInput(Identifier),
    /// A note or control change number MIDI does not have, or keys whose
    /// lowest note is above their highest.
    InputOutOfRange(Identifier),
    /// An OSC address that is not `/` followed by address characters.
    InvalidAddress(Identifier),
    /// A dial whose range is not two finite values from lower to higher.
    InvalidDialRange(Identifier),
    /// An OSC device listening on port 0.
    InvalidOscPort,
}

impl std::fmt::Display for InputDeviceValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateControl(id) => write!(f, "control {} appears twice", id.0),
            Self::InvalidControlName(name) => write!(
                f,
                "control `{}` must be a snake_case name unique within its device",
                name.as_str()
            ),
            Self::DuplicateInput(name) => write!(
                f,
                "control `{}` reads an input another control already reads",
                name.as_str()
            ),
            Self::ForeignInput(name) => write!(
                f,
                "control `{}` reads an input its device's driver does not have",
                name.as_str()
            ),
            Self::InputOutOfRange(name) => write!(
                f,
                "control `{}` reads a note above {MAX_MIDI_NOTE}, a control change above {MAX_MIDI_CONTROL_CHANGE}, or keys from a higher note to a lower one",
                name.as_str()
            ),
            Self::InvalidAddress(name) => write!(
                f,
                "control `{}` reads an OSC address that is not `/` followed by printable characters other than spaces and `*?[]{{}}#,`",
                name.as_str()
            ),
            Self::InvalidDialRange(name) => write!(
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
        let mut ids = std::collections::HashSet::new();
        let mut names = std::collections::HashSet::new();
        let mut inputs = std::collections::HashSet::new();
        if self.driver == (InputDriver::Osc { port: 0 }) {
            return Err(InputDeviceValidationError::InvalidOscPort);
        }
        for control in &self.controls {
            if !ids.insert(control.id) {
                return Err(InputDeviceValidationError::DuplicateControl(control.id));
            }
            if !donder_language::NameKind::Object.accepts(control.name.as_str())
                || !names.insert(&control.name)
            {
                return Err(InputDeviceValidationError::InvalidControlName(
                    control.name.clone(),
                ));
            }
            let sources = control.kind.sources();
            if sources.iter().any(|source| !self.driver.reads(source)) {
                return Err(InputDeviceValidationError::ForeignInput(
                    control.name.clone(),
                ));
            }
            if sources.iter().any(
                |source| matches!(source, InputSource::Address(address) if !address.is_valid()),
            ) {
                return Err(InputDeviceValidationError::InvalidAddress(
                    control.name.clone(),
                ));
            }
            if !control.kind.range_is_valid() {
                return Err(InputDeviceValidationError::InvalidDialRange(
                    control.name.clone(),
                ));
            }
            if sources.is_empty() || sources.iter().any(|source| !source.in_range()) {
                return Err(InputDeviceValidationError::InputOutOfRange(
                    control.name.clone(),
                ));
            }
            if !sources.into_iter().all(|source| inputs.insert(source)) {
                return Err(InputDeviceValidationError::DuplicateInput(
                    control.name.clone(),
                ));
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

pub type InputDeviceSource = crate::ownership::ValueSource<Box<InputDevice>, InputDeviceId>;

impl AsRef<ObjectIdentity> for InputDeviceId {
    fn as_ref(&self) -> &ObjectIdentity {
        &self.0
    }
}
