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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum InputDriver {
    Keyboard,
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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ButtonInput {
    Key(KeyCode),
}

/// A button of an input device.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ButtonRef {
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
            if !inputs.insert(control.kind) {
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
