//! Each xLights model type's nodes: where xLights draws each one in the
//! model's own coordinates, and its place in the model's channels.
//!
//! A node's channels are those of its string's start plus `offset` nodes.
//! Coordinates are xLights screen units before the model's placement.

mod arches;
mod circles;
mod custom;
mod icicles;
mod lines;
mod matrices;
mod star;
mod window;

use crate::placement::Placement;
use crate::report::SkipReason;
use crate::xml::Element;
use donder_runtime_types::PixelEncoding;
use glam::DVec3;

pub(crate) struct Node {
    pub(crate) string: u32,
    pub(crate) offset: u32,
    pub(crate) position: DVec3,
    pub(crate) arc: Option<ArcPoint>,
}

/// A node on one of the model's ellipses, at `degrees` along it.
#[derive(Clone, Copy)]
pub(crate) struct ArcPoint {
    pub(crate) arc: usize,
    pub(crate) degrees: f64,
}

/// The points `center + u cos φ + v sin φ`.
#[derive(Clone, Copy)]
pub(crate) struct Ellipse {
    pub(crate) center: DVec3,
    pub(crate) u: DVec3,
    pub(crate) v: DVec3,
}

pub(crate) struct Local {
    pub(crate) nodes: Vec<Node>,
    pub(crate) ellipses: Vec<Ellipse>,
    pub(crate) strings: u32,
    /// Nodes on each string, which sets where the next string starts.
    pub(crate) string_nodes: u32,
    pub(crate) placement: Placement,
}

/// A model's xLights settings, read as xLights reads them.
pub(crate) struct Spec<'a> {
    pub(crate) element: &'a Element,
    pub(crate) name: String,
    pub(crate) display: &'a str,
}

impl<'a> Spec<'a> {
    pub(crate) fn new(element: &'a Element) -> Self {
        Self {
            element,
            name: element
                .attribute("name")
                .unwrap_or_default()
                .trim()
                .to_string(),
            display: element.attribute("DisplayAs").unwrap_or_default(),
        }
    }

    fn text(&self, name: &str) -> Option<&'a str> {
        self.element.attribute(name)
    }

    fn invalid(&self, name: &str) -> SkipReason {
        SkipReason::Attribute {
            name: name.to_string(),
            value: self.text(name).unwrap_or_default().to_string(),
        }
    }

    pub(crate) fn float(&self, name: &str, default: f64) -> Result<f64, SkipReason> {
        match self
            .text(name)
            .map(str::trim)
            .filter(|text| !text.is_empty())
        {
            None => Ok(default),
            Some(text) => text
                .parse::<f64>()
                .ok()
                .filter(|value| value.is_finite())
                .ok_or_else(|| self.invalid(name)),
        }
    }

    pub(crate) fn int(&self, name: &str, default: i64) -> Result<i64, SkipReason> {
        match self
            .text(name)
            .map(str::trim)
            .filter(|text| !text.is_empty())
        {
            None => Ok(default),
            Some(text) => text.parse().map_err(|_| self.invalid(name)),
        }
    }

    /// A count written under its own name since xLights 2026.04, and as
    /// `parm1`, `parm2` or `parm3` before.
    pub(crate) fn count(&self, name: &str, parm: &str, default: i64) -> Result<u32, SkipReason> {
        let attribute = if self.text(name).is_some() {
            name
        } else {
            parm
        };
        let value = self.int(attribute, default)?;
        u32::try_from(value).map_err(|_| self.invalid(attribute))
    }

    pub(crate) fn is_true(&self, name: &str, default: bool) -> bool {
        self.text(name)
            .map_or(default, |text| matches!(text, "true" | "1" | "TRUE"))
    }

    pub(crate) fn left_to_right(&self) -> bool {
        self.text("Dir") != Some("R")
    }

    pub(crate) fn bottom_to_top(&self, default: bool) -> bool {
        self.text("StartSide").map_or(default, |side| side != "T")
    }

    pub(crate) fn has(&self, name: &str) -> bool {
        self.text(name).is_some_and(|text| !text.trim().is_empty())
    }

    pub(crate) fn csv(&self, name: &str) -> Result<Vec<i64>, SkipReason> {
        self.text(name)
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(|item| item.parse().map_err(|_| self.invalid(name)))
            .collect()
    }
}

/// How a node's colors are sent, and in how many channels.
pub(crate) fn encoding(spec: &Spec) -> Result<PixelEncoding, SkipReason> {
    let text = spec.text("StringType").unwrap_or("RGB Nodes");
    let index = |letter| match letter {
        'R' => Some(0),
        'G' => Some(1),
        'B' => Some(2),
        'W' => Some(3),
        _ => None,
    };
    let letters = text
        .strip_suffix(" Nodes")
        .map(|letters| letters.chars().map(index).collect::<Option<Vec<u8>>>());
    let encoding = match letters {
        Some(Some(order)) => match order.as_slice() {
            [a, b, c] => Some(PixelEncoding::Rgb {
                order: [*a, *b, *c],
            }),
            [a, b, c, d] => Some(PixelEncoding::Rgbw {
                order: [*a, *b, *c, *d],
            }),
            _ => None,
        },
        _ => None,
    };
    encoding
        .filter(PixelEncoding::is_valid)
        .ok_or_else(|| SkipReason::StringType(text.to_string()))
}

/// The mean of a node's light positions.
pub(crate) fn mean(points: &[DVec3]) -> DVec3 {
    points.iter().copied().sum::<DVec3>() / points.len().max(1) as f64
}

/// Why the model is not imported, before its nodes are laid out.
pub(crate) fn excluded(spec: &Spec) -> Result<(), SkipReason> {
    if spec.has("ShadowModelFor") {
        return Err(SkipReason::Shadow);
    }
    if spec.int("Active", 1)? == 0 {
        return Err(SkipReason::Inactive);
    }
    Ok(())
}

/// The channels a model takes that has no pixels Donder can show.
pub(crate) fn channel_count(spec: &Spec) -> Option<u32> {
    if spec.display.starts_with("Dmx") || spec.display == "DMX" {
        spec.count("DmxChannelCount", "parm1", 1).ok()
    } else if spec.display == "Channel Block" {
        spec.count("NumChannels", "parm1", 1).ok()
    } else {
        None
    }
}

pub(crate) fn layout(spec: &Spec) -> Result<Local, SkipReason> {
    if spec.int("versionNumber", 0)? < 2 {
        return Err(SkipReason::OldLayout);
    }
    if spec.is_true("Advanced", false) {
        return Err(SkipReason::Setting("individual string start channel"));
    }
    let local = match spec.display {
        "Single Line" => lines::single_line(spec),
        "Poly Line" => lines::poly_line(spec),
        "Arches" => arches::arches(spec),
        "Circle" => circles::circle(spec),
        "Wreath" => circles::wreath(spec),
        "Star" => star::star(spec),
        "Matrix" | "Horiz Matrix" | "Vert Matrix" => matrices::matrix(spec),
        "Tree" => matrices::tree(spec),
        display if display.starts_with("Tree ") => matrices::tree(spec),
        "Sphere" => matrices::sphere(spec),
        "Icicles" => icicles::icicles(spec),
        "Window Frame" => window::window_frame(spec),
        "Custom" => custom::custom(spec),
        other => Err(SkipReason::Type(other.to_string())),
    }?;
    if local.nodes.is_empty() {
        return Err(SkipReason::NoPixels);
    }
    Ok(local)
}
