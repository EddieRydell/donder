//! What a prepared show's memory holds: its bytes and its playback buffer,
//! by part, so a show too large for a controller can say why.
use crate::format::Sections;
use alloc::vec::Vec;
use core::mem::size_of_val;

/// A part of a prepared show's memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShowPart {
    /// Clips, layers, graph nodes, their parameters and automation.
    Timeline,
    /// The compiled effect and operator programs.
    Programs,
    /// Curve points and their precomputed areas, slopes and crossings.
    Curves,
    Gradients,
    /// Mark times and durations.
    Marks,
    /// Audio tracks' samples.
    Tracks,
    /// Fonts' glyphs.
    Fonts,
    /// Images' pixels.
    Images,
    /// Array parameters' items.
    Arrays,
    /// Fixtures, pixel positions, targets and output routes.
    Layout,
    /// A console's actions, voices and live bindings.
    Console,
    /// The show's header, section table and alignment.
    Format,
    /// Playback: one frame of colors per layer or operator lit at once, and
    /// per scan.
    Frames,
    /// Playback: the strip interpreter's values.
    Slots,
    /// Playback: output bytes, clip caches and console state.
    PlaybackState,
}

/// The bytes of each part of a show, largest first.
#[derive(Clone, Debug, PartialEq)]
pub struct ShowSize {
    pub parts: Vec<(ShowPart, usize)>,
    /// Pixels the show drives, which every frame buffer holds.
    pub pixels: usize,
    /// Frames playback holds at once: layers and operators lit together, and
    /// scans.
    pub frames: usize,
    pub clips: usize,
}

impl ShowSize {
    pub fn total(&self) -> usize {
        self.parts.iter().map(|(_, bytes)| bytes).sum()
    }

    pub fn bytes(&self, part: ShowPart) -> usize {
        self.parts
            .iter()
            .find(|(candidate, _)| *candidate == part)
            .map_or(0, |(_, bytes)| *bytes)
    }
}

/// Bytes of each part of the show sections `s`, whose encoding takes
/// `encoded` bytes; the remainder is the format's own.
pub(crate) fn show_parts(s: &Sections<'_>, encoded: usize) -> Vec<(ShowPart, usize)> {
    let parts = [
        (
            ShowPart::Timeline,
            size_of_val(s.clips)
                + size_of_val(s.clip_index)
                + size_of_val(s.layers)
                + size_of_val(s.layer_order)
                + size_of_val(s.layer_ends)
                + size_of_val(s.nodes)
                + size_of_val(s.node_inputs)
                + size_of_val(s.param_sets)
                + size_of_val(s.words)
                + size_of_val(s.colors)
                + size_of_val(s.resources)
                + size_of_val(s.automation)
                + size_of_val(s.bindings)
                + size_of_val(s.enum_options)
                + size_of_val(s.windows),
        ),
        (
            ShowPart::Programs,
            size_of_val(s.programs) + size_of_val(s.code) + size_of_val(s.program_resources),
        ),
        (
            ShowPart::Curves,
            size_of_val(s.curves)
                + size_of_val(s.curve_points)
                + size_of_val(s.curve_areas)
                + size_of_val(s.curve_slopes)
                + size_of_val(s.crossings),
        ),
        (
            ShowPart::Gradients,
            size_of_val(s.gradients) + size_of_val(s.gradient_stops),
        ),
        (
            ShowPart::Marks,
            size_of_val(s.marks) + size_of_val(s.mark_ticks),
        ),
        (
            ShowPart::Tracks,
            size_of_val(s.audio_tracks) + size_of_val(s.audio_samples),
        ),
        (
            ShowPart::Fonts,
            size_of_val(s.fonts) + size_of_val(s.glyphs) + size_of_val(s.glyph_coverage),
        ),
        (
            ShowPart::Images,
            size_of_val(s.images) + size_of_val(s.image_pixels),
        ),
        (
            ShowPart::Arrays,
            size_of_val(s.arrays) + size_of_val(s.values),
        ),
        (
            ShowPart::Layout,
            size_of_val(s.meta)
                + size_of_val(s.fixtures)
                + size_of_val(s.positions)
                + size_of_val(s.targets)
                + size_of_val(s.pixel_runs)
                + size_of_val(s.pixels)
                + size_of_val(s.spatial_runs)
                + size_of_val(s.section_runs)
                + size_of_val(s.section_spans)
                + size_of_val(s.section_pixels)
                + size_of_val(s.routes)
                + size_of_val(s.lookups)
                + size_of_val(s.palettes)
                + size_of_val(s.outputs),
        ),
        (
            ShowPart::Console,
            size_of_val(s.voices)
                + size_of_val(s.live_automations)
                + size_of_val(s.live_targets)
                + size_of_val(s.console_ons)
                + size_of_val(s.console_actions)
                + size_of_val(s.played_values)
                + size_of_val(s.console_clips)
                + size_of_val(s.console_indices)
                + size_of_val(s.console_always),
        ),
    ];
    let sections: usize = parts.iter().map(|(_, bytes)| bytes).sum();
    let mut parts = parts.to_vec();
    parts.push((ShowPart::Format, encoded.saturating_sub(sections)));
    parts
}
