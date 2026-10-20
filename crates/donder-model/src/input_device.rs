use crate::identity::ObjectIdentity;
use donder_runtime_types::Identifier;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct InputDeviceId(pub ObjectIdentity);

/// A control of an input device, numbered from 1 in authored order.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct InputControlId(pub u32);

/// Hardware a live performance plays from: its controls, each bound to one
/// input of its driver.
#[derive(Clone, Debug, Eq, PartialEq)]
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
}

impl InputDriver {
    /// Whether this driver reads `input`.
    pub fn reads(&self, input: InputSource) -> bool {
        match self {
            Self::Keyboard => matches!(input, InputSource::Key(_)),
            Self::Midi { .. } => !matches!(input, InputSource::Key(_)),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputControl {
    pub id: InputControlId,
    /// Unique within the device; rigs refer to it.
    pub name: Identifier,
    pub description: Option<String>,
    pub kind: InputControlKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum InputControlKind {
    /// Pressed or released.
    Button(ButtonInput),
    /// A position from 0 to 1.
    Dial(DialInput),
}

/// The highest MIDI note number.
pub const MAX_MIDI_NOTE: u8 = 127;
/// The highest MIDI control change number; 120 and up are channel modes.
pub const MAX_MIDI_CONTROL_CHANGE: u8 = 119;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ButtonInput {
    Key(KeyCode),
    /// A MIDI note, held from note on to note off.
    Note(u8),
    /// A MIDI control change, held at 64 and above, as a sustain pedal is.
    ControlChange(u8),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum DialInput {
    /// A MIDI control change, from 0 to 127.
    ControlChange(u8),
    /// The MIDI pitch bend wheel, from fully down to fully up.
    PitchBend,
}

/// What a control reads, whichever kind of control reads it. No two controls
/// of a device read the same input.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum InputSource {
    Key(KeyCode),
    Note(u8),
    ControlChange(u8),
    PitchBend,
}

impl InputControlKind {
    pub fn source(self) -> InputSource {
        match self {
            Self::Button(ButtonInput::Key(code)) => InputSource::Key(code),
            Self::Button(ButtonInput::Note(note)) => InputSource::Note(note),
            Self::Button(ButtonInput::ControlChange(number))
            | Self::Dial(DialInput::ControlChange(number)) => InputSource::ControlChange(number),
            Self::Dial(DialInput::PitchBend) => InputSource::PitchBend,
        }
    }

    pub fn is_button(self) -> bool {
        matches!(self, Self::Button(_))
    }
}

impl InputSource {
    /// Whether the number it reads is one MIDI has.
    pub fn in_range(self) -> bool {
        match self {
            Self::Key(_) | Self::PitchBend => true,
            Self::Note(note) => note <= MAX_MIDI_NOTE,
            Self::ControlChange(number) => number <= MAX_MIDI_CONTROL_CHANGE,
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
        }
    }
}

/// A control of an input device: a button, or a dial.
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
    /// A note or control change number MIDI does not have.
    InputOutOfRange(Identifier),
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
                "control `{}` reads a note above {MAX_MIDI_NOTE} or a control change above {MAX_MIDI_CONTROL_CHANGE}",
                name.as_str()
            ),
        }
    }
}

impl InputDevice {
    pub fn validate(&self) -> Result<(), InputDeviceValidationError> {
        let mut ids = std::collections::HashSet::new();
        let mut names = std::collections::HashSet::new();
        let mut inputs = std::collections::HashSet::new();
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
            let source = control.kind.source();
            if !self.driver.reads(source) {
                return Err(InputDeviceValidationError::ForeignInput(
                    control.name.clone(),
                ));
            }
            if !source.in_range() {
                return Err(InputDeviceValidationError::InputOutOfRange(
                    control.name.clone(),
                ));
            }
            if !inputs.insert(source) {
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
