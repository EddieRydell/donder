#![deny(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unwrap_used
    )
)]

mod dto {
    pub use donder_sequence_api::*;
}
pub mod gui;
pub mod source_documents;
pub use gui::*;
