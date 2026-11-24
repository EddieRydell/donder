//! The coded pattern a fixture capture films: after a dark pause and a lit
//! slot, every pixel blinks the bits of its index's Gray code, each bit
//! followed by its inverse, so a camera can tell each pixel apart.

/// Seconds each slot of the pattern lasts.
pub const CAPTURE_SLOT_SECONDS: f32 = 0.2;
/// Dark slots that open each cycle. Inside a cycle at most two slots in a
/// row are nearly dark (an inverted bit, then the next bit).
const LEAD_SLOTS: u32 = 4;
/// All-lit slots after the lead, which the decoder synchronizes on.
const LIT_SLOTS: u32 = 2;
/// The dark slot between the lit slots and the first bit.
const GAP_SLOTS: u32 = 1;

/// What every pixel shows during one slot of a cycle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureSlot {
    Dark,
    Lit,
    /// Pixels whose code has `bit` set light, or those without it when
    /// `inverted`.
    Bit {
        bit: u32,
        inverted: bool,
    },
}

/// The coded pattern for a fixture's pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureCode {
    pixels: u32,
    bits: u32,
}

impl CaptureCode {
    pub fn new(pixels: u32) -> Self {
        Self {
            pixels,
            bits: (u32::BITS - pixels.saturating_sub(1).leading_zeros()).max(1),
        }
    }

    pub fn pixels(&self) -> u32 {
        self.pixels
    }

    pub fn bits(&self) -> u32 {
        self.bits
    }

    /// Slots in one cycle.
    pub fn slots(&self) -> u32 {
        LEAD_SLOTS + LIT_SLOTS + GAP_SLOTS + 2 * self.bits
    }

    pub fn cycle_seconds(&self) -> f32 {
        self.slots() as f32 * CAPTURE_SLOT_SECONDS
    }

    /// The slot the decoder synchronizes on: the first lit one.
    pub fn first_lit_slot(&self) -> u32 {
        LEAD_SLOTS
    }

    pub fn slot(&self, index: u32) -> CaptureSlot {
        let bit = index.checked_sub(LEAD_SLOTS + LIT_SLOTS + GAP_SLOTS);
        match bit {
            Some(bit) => CaptureSlot::Bit {
                bit: bit / 2,
                inverted: bit % 2 == 1,
            },
            None if (LEAD_SLOTS..LEAD_SLOTS + LIT_SLOTS).contains(&index) => CaptureSlot::Lit,
            None => CaptureSlot::Dark,
        }
    }

    /// Whether `pixel` is lit `seconds` into the repeating pattern.
    pub fn lit(&self, pixel: u32, seconds: f32) -> bool {
        let slot = (seconds / CAPTURE_SLOT_SECONDS) as u32 % self.slots();
        match self.slot(slot) {
            CaptureSlot::Dark => false,
            CaptureSlot::Lit => true,
            CaptureSlot::Bit { bit, inverted } => ((gray(pixel) >> bit) & 1 == 1) != inverted,
        }
    }

    /// The pixel a decoded code names, if the fixture has it.
    pub fn pixel(&self, code: u32) -> Option<u32> {
        let mut pixel = code;
        let mut shift = code >> 1;
        while shift != 0 {
            pixel ^= shift;
            shift >>= 1;
        }
        (pixel < self.pixels).then_some(pixel)
    }
}

/// Neighboring pixels' codes differ in one bit.
fn gray(pixel: u32) -> u32 {
    pixel ^ (pixel >> 1)
}
