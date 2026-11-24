//! Measure a fixture's shape from a phone video of its pixels playing the
//! coded capture pattern: decode the video, find where each pixel lit, and
//! fit the simplest fixture shapes that explain those points.
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

mod fit;
mod pattern;
mod video;

use core::fmt;

use donder_language::DistanceSpan;
use donder_model::{FixtureDefinition, FixtureElement};
use donder_output::CaptureCode;
use glam::Vec2;

/// Meters between neighboring pixels of a fixture that had no size yet.
const DEFAULT_SPACING_METERS: f32 = 0.1;
/// Pixel diameter, as a share of the spacing, for a fixture with no shapes yet.
const DEFAULT_DIAMETER_SPACINGS: f32 = 0.5;

/// The shapes measured for a fixture's pixels.
#[derive(Debug)]
pub struct FixtureCapture {
    /// Every pixel in output order, sized like the fixture's current pixels.
    pub elements: Vec<FixtureElement>,
    /// Pixels seen in the video; the rest were placed between their neighbors.
    pub found: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CaptureError {
    NotVideo,
    UnsupportedCodec,
    Decode(String),
    NoPattern,
    TooShort,
    FrameRate,
    TooFewLights { found: u32 },
    TooLarge,
}

impl fmt::Display for CaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotVideo => write!(f, "The file isn't an MP4 or MOV video."),
            Self::UnsupportedCodec => write!(f, "Only H.264 and HEVC videos can be read."),
            Self::Decode(error) => write!(f, "The video can't be decoded: {error}."),
            Self::NoPattern => write!(
                f,
                "The pattern wasn't found. Keep the lights in view and lock the camera's exposure."
            ),
            Self::TooShort => write!(f, "Film until the pattern has repeated twice."),
            Self::FrameRate => write!(f, "Film at 24 frames per second or more."),
            Self::TooFewLights { found } => write!(
                f,
                "Only {found} lights were found. Film closer, in the dark, with the exposure locked."
            ),
            Self::TooLarge => write!(f, "The measured shape is too large for a fixture."),
        }
    }
}

/// Measure `current`'s replacement shapes from `video`, which films
/// `pixels` pixels playing the capture pattern. The result keeps the current
/// pixels' center, size and diameter; a fixture without them is centered at
/// its origin with default spacing.
pub fn capture_fixture(
    video: &[u8],
    pixels: u32,
    current: &FixtureDefinition,
) -> Result<FixtureCapture, CaptureError> {
    let code = CaptureCode::new(pixels);
    let points = pattern::locate(&video::Video::open(video)?, code)?;
    let found = points.iter().filter(|point| point.is_some()).count() as u32;
    let elements =
        fit::fit(&points, &placement(current)).ok_or(CaptureError::TooFewLights { found })?;
    if elements
        .iter()
        .any(|element| donder_model::element_pixels(element).is_err())
    {
        return Err(CaptureError::TooLarge);
    }
    Ok(FixtureCapture { elements, found })
}

fn placement(current: &FixtureDefinition) -> fit::Placement {
    let pixels = donder_model::prepare_geometry(current);
    let (low, high) = pixels.iter().fold(
        (Vec2::INFINITY, Vec2::NEG_INFINITY),
        |(low, high), pixel| {
            (
                low.min(pixel.position.truncate()),
                high.max(pixel.position.truncate()),
            )
        },
    );
    let diagonal = low.distance(high);
    let sized = pixels.len() > 1 && diagonal > 0.0;
    fit::Placement {
        center: if sized {
            (low + high) / 2.0
        } else {
            Vec2::ZERO
        },
        diagonal: sized.then_some(diagonal),
        spacing: DEFAULT_SPACING_METERS,
        diameter: current.elements.first().map_or(
            DistanceSpan::from_meters(DEFAULT_DIAMETER_SPACINGS * DEFAULT_SPACING_METERS),
            |element| element.diameter,
        ),
    }
}
