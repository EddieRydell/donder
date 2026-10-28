//! Print a sequence's fixture colors at chosen times, so a show can be checked
//! without the Preview.
use core::time::Duration;
use std::collections::HashSet;
use std::fmt::Write;

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
    let project = &session.project;
    let name = |id: donder_model::ObjectId| {
        session
            .address_of(id)
            .map(|address| address.declared_name().to_string())
            .ok_or("An object is missing from the source inventory.")
    };
    let setup = project
        .get::<donder_model::Setup>(*project.root().setup.id())
        .ok_or("Project setup is missing.")?;
    let layout = project
        .get::<donder_model::Layout>(*setup.layout.id())
        .ok_or("Setup layout is missing.")?;
    let selected = selected_fixtures(layout, &name(layout.id.into())?, targets)?;
    let prepared = donder_elaboration::prepare(project, id, donder_elaboration::Retain::All)
        .ok_or("The sequence cannot be played on the active setup.")?;
    let rate = prepared.frame_rate();
    let frame_count = prepared.frame_count();
    // Each fixture's label, or `None` when it is not selected.
    let labels = prepared
        .fixtures()
        .iter()
        .map(|fixture| {
            let name = layout
                .fixture(FixtureInstanceId(fixture.id))
                .map(|fixture| fixture.name.as_str())
                .ok_or("A prepared fixture is missing from the layout.")?;
            Ok(selected
                .as_ref()
                .is_none_or(|ids| ids.contains(&fixture.id))
                .then(|| format!("{name} ({}):", fixture.pixel_count)))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let width = labels.iter().flatten().map(String::len).max().unwrap_or(0);
    let sequence = name((*id).into())?;
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
            if let Some(label) = label {
                let _ = writeln!(text, "  {label:<width$} {}", runs(fixture.pixels));
            }
        }
    }
    Ok(text)
}

/// The placed fixtures the named fixtures and groups denote; `None` selects all.
fn selected_fixtures(
    layout: &Layout,
    layout_name: &str,
    targets: &[String],
) -> Result<Option<HashSet<u32>>, String> {
    if targets.is_empty() {
        return Ok(None);
    }
    let mut selected = HashSet::new();
    for target in targets {
        let fixture = layout
            .iter_fixtures()
            .find(|fixture| fixture.name.as_str() == target)
            .ok_or_else(|| {
                format!("The layout `{layout_name}` has no fixture or group named `{target}`.")
            })?;
        selected.extend(layout.members(fixture.id).into_iter().map(|id| id.0));
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
