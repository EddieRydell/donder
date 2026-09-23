#![deny(unsafe_code)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
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

fn main() {
    if let Err(error) = donder_desktop::run() {
        eprintln!("failed to run Donder desktop: {error}");
        std::process::exit(1);
    }
}
