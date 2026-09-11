use crate::RenderError;

#[derive(Clone, Debug, PartialEq)]
pub enum SequenceOutputPrepareError {
    ProjectValidation(String),
    Render(RenderError),
    MissingSetup,
    MissingLayout,
    MissingSequence,
    InvalidEffectLayout,
    UnknownOutput {
        controller: dawn_language::controller::ControllerId,
        port: dawn_language::controller::ControllerPortId,
    },
    DuplicateOutput {
        controller: dawn_language::controller::ControllerId,
        port: dawn_language::controller::ControllerPortId,
    },
    InvalidPatch(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum SequenceOutputRenderError {
    Render(RenderError),
    Patch(String),
}

impl From<dawn_runtime::sequence::SequenceError> for SequenceOutputRenderError {
    fn from(error: dawn_runtime::sequence::SequenceError) -> Self {
        use dawn_runtime::sequence::SequenceError;
        match error {
            SequenceError::InvalidWorkspace => {
                Self::Patch("evaluation workspace belongs to another prepared output".to_string())
            }
            SequenceError::Signal(error) => Self::Render(error.into()),
            SequenceError::Patch(error) => Self::Patch(format!("{error:?}")),
        }
    }
}
