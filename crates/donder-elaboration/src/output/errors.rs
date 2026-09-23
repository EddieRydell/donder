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
        controller: donder_language::controller::ControllerId,
        port: donder_language::controller::ControllerPortId,
    },
    DuplicateOutput {
        controller: donder_language::controller::ControllerId,
        port: donder_language::controller::ControllerPortId,
    },
    InvalidPatch(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum SequenceOutputRenderError {
    Render(RenderError),
    Patch(String),
}

impl From<donder_runtime::sequence::SequenceError> for SequenceOutputRenderError {
    fn from(error: donder_runtime::sequence::SequenceError) -> Self {
        use donder_runtime::sequence::SequenceError;
        match error {
            SequenceError::InvalidWorkspace => {
                Self::Patch("evaluation workspace belongs to another prepared output".to_string())
            }
            SequenceError::Signal(error) => Self::Render(error.into()),
            SequenceError::Patch(error) => Self::Patch(format!("{error:?}")),
        }
    }
}
