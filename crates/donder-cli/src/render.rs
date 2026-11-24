//! Print a sequence's fixture colors at chosen times, so a show can be checked
//! without the Preview.
use core::time::Duration;
use std::collections::HashMap;
use std::fmt::Write;
use std::ops::Range;

use donder_language::data::canonical_duration;
use donder_model::{FixtureInstanceId, Layout, SequenceId};
use donder_project_io::ProjectSession;
use donder_runtime_types::{Color, sample_time_from_frame};

const NANOS_PER_SECOND: u128 = 1_000_000_000;

/// Times to sample: each `at`, then `from` through `to` every `step`.
pub(crate) struct Times {
    pub(crate) at: Vec<Duration>,
    pub(crate) range: Option<(Duration, Duration, Duration)>,
}

impl Times {
    fn expand(self) -> Result<Vec<Duration>, String> {
        let mut times = self.at;
        if let Some((from, to, step)) = self.range {
            if step.is_zero() {
                return Err("--step must be longer than 0s.".into());
            }
            let mut time = from;
            while time <= to {
                times.push(time);
                time += step;
            }
        }
        if times.is_empty() {
            return Err("Give a time with --at, or a range with --from, --to and --step.".into());
        }
        Ok(times)
    }
}

pub(crate) fn render(
    session: &ProjectSession,
    id: &SequenceId,
    times: Times,
    targets: &[String],
) -> Result<String, String> {
    let times = times.expand()?;
    let project = session.project();
    let name = |id: donder_model::ObjectId| session.address_of(id).declared_name().to_string();
    let layout = project.active_layout().object();
    let selected = selected_fixtures(layout, &name(layout.id.into()), targets)?;
    let prepared = donder_elaboration::prepare(project, id, donder_elaboration::Retain::All)
        .ok_or("The sequence cannot be played on the active setup.")?;
    let rate = prepared.frame_rate();
    let frame_count = prepared.frame_count();
    // Each fixture's label and selected pixels, or `None` when none are.
    let labels = prepared
        .fixtures()
        .iter()
        .map(|fixture| {
            let name = layout
                .fixture(FixtureInstanceId(fixture.id))
                .map(|fixture| fixture.name.as_str())
                .ok_or("A prepared fixture is missing from the layout.")?;
            let pixels: Vec<usize> = match &selected {
                None => (0..fixture.pixel_count as usize).collect(),
                Some(parts) => parts.get(&fixture.id).map_or_else(Vec::new, |spans| {
                    (0..fixture.pixel_count)
                        .filter(|pixel| spans.iter().any(|span| span.contains(pixel)))
                        .map(|pixel| pixel as usize)
                        .collect()
                }),
            };
            Ok((!pixels.is_empty()).then(|| (format!("{name} ({}):", pixels.len()), pixels)))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let width = labels
        .iter()
        .flatten()
        .map(|(label, _)| label.len())
        .max()
        .unwrap_or(0);
    let sequence = name((*id).into());
    let mut playback = prepared.into_playback();
    let mut text = String::new();
    for time in times {
        let time_text = canonical_duration(time);
        let past_end = || format!("{time_text} is past the end of `{sequence}`.");
        let frame = u32::try_from(time.as_nanos() * u128::from(rate) / NANOS_PER_SECOND)
            .ok()
            .filter(|&frame| frame < frame_count)
            .ok_or_else(past_end)?;
        let sample_time = sample_time_from_frame(frame, rate).map_err(|_| past_end())?;
        let _ = writeln!(text, "{sequence} @ {time_text} (frame {frame})");
        for (label, fixture) in labels.iter().zip(playback.evaluate(sample_time).fixtures()) {
            if let Some((label, pixels)) = label {
                let colors: Vec<Color> =
                    pixels.iter().map(|&pixel| fixture.pixels[pixel]).collect();
                let _ = writeln!(text, "  {label:<width$} {}", runs(&colors));
            }
        }
    }
    Ok(text)
}

/// Each placed fixture's pixel spans a selection denotes, by fixture id.
type Selected = HashMap<u32, Vec<Range<u32>>>;

/// Each placed fixture's pixel spans the named fixtures and groups denote;
/// `None` selects all.
fn selected_fixtures(
    layout: &Layout,
    layout_name: &str,
    targets: &[String],
) -> Result<Option<Selected>, String> {
    if targets.is_empty() {
        return Ok(None);
    }
    let mut selected = Selected::new();
    for target in targets {
        let fixture = layout
            .iter_fixtures()
            .find(|fixture| fixture.name.as_str() == target)
            .ok_or_else(|| {
                format!("The layout `{layout_name}` has no fixture or group named `{target}`.")
            })?;
        for part in layout.members(fixture.id) {
            let span = part.pixels.map_or(0..u32::MAX, |span| {
                span.start..span.start.saturating_add(span.count)
            });
            selected.entry(part.fixture.0).or_default().push(span);
        }
    }
    Ok(Some(selected))
}

/// Colors in pixel order as 8-bit hex, each run of one color written once
/// with its length.
fn runs(pixels: &[Color]) -> String {
    let mut text = String::new();
    for run in pixels.chunk_by(|a, b| a.to_bytes() == b.to_bytes()) {
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(&run[0].to_hex());
        if run.len() > 1 {
            let _ = write!(text, "×{}", run.len());
        }
    }
    text
}
