//! Whether a prepared show fits a Donder controller, and if not, what to
//! change.
use donder_device_protocol::SHOW_REGION_BYTES;
use donder_runtime::{ShowPart, ShowSize};

/// The largest parts an oversized show's message names.
const NAMED_PARTS: usize = 3;

/// Why a show of `size` does not fit a controller, or `None` when it does.
pub(crate) fn too_large(size: &ShowSize) -> Option<String> {
    let total = size.total();
    if total <= SHOW_REGION_BYTES {
        return None;
    }
    let parts = size
        .parts
        .iter()
        .filter(|(_, bytes)| *bytes > 0)
        .take(NAMED_PARTS)
        .map(|&(part, bytes)| format!("{} {} ({})", what(part, size), kilobytes(bytes), fix(part)))
        .collect::<Vec<_>>()
        .join("; ");
    Some(format!(
        "This show needs {} but a Donder controller holds {}. Largest: {parts}.",
        kilobytes(total),
        kilobytes(SHOW_REGION_BYTES),
    ))
}

fn kilobytes(bytes: usize) -> String {
    format!("{:.1} KB", bytes as f64 / 1024.0)
}

fn what(part: ShowPart, size: &ShowSize) -> String {
    match part {
        ShowPart::Timeline => format!("{} clips with their parameters and automation", size.clips),
        ShowPart::Programs => "compiled effects and operators".into(),
        ShowPart::Curves => "curves".into(),
        ShowPart::Gradients => "gradients".into(),
        ShowPart::Marks => "marks".into(),
        ShowPart::Tracks => "audio tracks".into(),
        ShowPart::Fonts => "fonts".into(),
        ShowPart::Images => "images".into(),
        ShowPart::Arrays => "array parameters".into(),
        ShowPart::Layout => format!("{} pixels' positions and routes", size.pixels),
        ShowPart::Console => "console actions and bindings".into(),
        ShowPart::Format => "the show's format".into(),
        ShowPart::Frames => format!(
            "{} frames of {} pixels for the layers, operators and scans lit at once",
            size.frames, size.pixels
        ),
        ShowPart::Slots => "effect working values".into(),
        ShowPart::PlaybackState => "output and clip playback state".into(),
    }
}

fn fix(part: ShowPart) -> &'static str {
    match part {
        ShowPart::Timeline => "split the sequence or remove clips",
        ShowPart::Programs => "use fewer distinct effects and operators",
        ShowPart::Curves => "use curves with fewer points",
        ShowPart::Gradients => "use gradients with fewer stops",
        ShowPart::Marks => "use smaller mark collections",
        ShowPart::Tracks => "lower the tracks' rates or bands",
        ShowPart::Fonts => "use fewer fonts or smaller heights",
        ShowPart::Images => "use fewer or smaller images, or animations with fewer frames",
        ShowPart::Arrays => "shorten array parameters",
        ShowPart::Layout | ShowPart::PlaybackState => "move fixtures to another controller",
        ShowPart::Console => "remove console actions",
        ShowPart::Format => "it grows with the other parts",
        ShowPart::Frames => {
            "merge layers, light fewer at once, or move fixtures to another controller"
        }
        ShowPart::Slots => "simplify the largest effect",
    }
}
