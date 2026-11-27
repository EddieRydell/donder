//! An effect's settings and color palette as xLights stores them: comma
//! separated `key=value` pairs, read as xLights' `SettingsMap` reads them.

use donder_runtime_types::Color;
use std::collections::HashMap;

const WHITE: Color = Color {
    red: 1.0,
    green: 1.0,
    blue: 1.0,
};

/// `key=value` pairs; xLights writes a comma inside a value as `&comma;`.
fn pairs(text: &str) -> HashMap<String, String> {
    text.split(',')
        .filter_map(|pair| pair.split_once('='))
        .map(|(key, value)| (key.trim().to_owned(), value.replace("&comma;", ",")))
        .collect()
}

/// An effect's settings: `E_` effect, `B_` buffer and `T_` layer settings.
#[derive(Default)]
pub(crate) struct Settings(HashMap<String, String>);

impl Settings {
    pub(crate) fn parse(text: &str) -> Self {
        Self(pairs(text))
    }

    pub(crate) fn text(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(String::as_str)
    }

    pub(crate) fn choice<'a>(&'a self, key: &str, default: &'a str) -> &'a str {
        self.text(key).unwrap_or(default)
    }

    pub(crate) fn float(&self, key: &str, default: f32) -> f32 {
        self.text(key)
            .and_then(|value| value.trim().parse::<f32>().ok())
            .filter(|value| value.is_finite())
            .unwrap_or(default)
    }

    pub(crate) fn bool(&self, key: &str, default: bool) -> bool {
        self.text(key).map_or(default, |value| {
            matches!(value.trim(), "1" | "TRUE" | "true")
        })
    }

    /// The types of the active value curves the importer cannot evaluate.
    pub(crate) fn unknown_curves(&self) -> impl Iterator<Item = String> + '_ {
        self.0
            .iter()
            .filter(|(key, _)| key.contains("VALUECURVE"))
            .filter_map(|(_, value)| unknown_curve(value))
    }

    /// The value curve set on `key`'s slider, when it is active: xLights
    /// writes it as `E_VALUECURVE_<name>` beside `E_SLIDER_<name>` or
    /// `E_TEXTCTRL_<name>`.
    pub(crate) fn curve(&self, name: &str) -> Option<ValueCurve> {
        ValueCurve::parse(self.text(&format!("E_VALUECURVE_{name}"))?)
    }
}

/// A slider's value over the effect: xLights' `ValueCurve`, in the slider's
/// own units.
#[derive(Clone, Debug)]
pub(crate) struct ValueCurve {
    kind: CurveKind,
    min: f32,
    max: f32,
    /// P1..P4, in 0..100 of the slider's range, as xLights stores them.
    p: [f32; 4],
    /// Custom points, `(x, y)` both in 0..1.
    points: Vec<(f32, f32)>,
    /// Whether the slider's values were divided to fit the curve (`RV`).
    real_values: bool,
    /// Whether a parabola is scaled between P3 and P4 (`SE`).
    ends: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum CurveKind {
    Flat,
    Ramp,
    RampUpDown,
    RampUpDownHold,
    Saw,
    Square,
    Sine,
    AbsSine,
    ParabolicDown,
    ParabolicUp,
    LogarithmicUp,
    LogarithmicDown,
    ExponentialUp,
    ExponentialDown,
    Custom,
    Random,
}

impl CurveKind {
    /// The curve types xLights names that the importer evaluates.
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "Flat" => Self::Flat,
            "Ramp" => Self::Ramp,
            "Ramp Up/Down" => Self::RampUpDown,
            "Ramp Up/Down Hold" => Self::RampUpDownHold,
            "Saw Tooth" => Self::Saw,
            "Square" => Self::Square,
            "Sine" => Self::Sine,
            "Abs Sine" => Self::AbsSine,
            "Parabolic Down" => Self::ParabolicDown,
            "Parabolic Up" => Self::ParabolicUp,
            "Logarithmic Up" => Self::LogarithmicUp,
            "Logarithmic Down" => Self::LogarithmicDown,
            "Exponential Up" => Self::ExponentialUp,
            "Exponential Down" => Self::ExponentialDown,
            "Custom" => Self::Custom,
            "Random" => Self::Random,
            _ => return None,
        })
    }
}

/// The type of an active value curve the importer cannot evaluate, if
/// `text` is one.
pub(crate) fn unknown_curve(text: &str) -> Option<String> {
    let fields = text
        .split('|')
        .filter_map(|field| field.split_once('='))
        .collect::<HashMap<_, _>>();
    if fields.get("Active").copied() != Some("TRUE") {
        return None;
    }
    let name = fields.get("Type").copied().unwrap_or("Flat");
    CurveKind::parse(name).is_none().then(|| name.to_owned())
}

impl ValueCurve {
    fn parse(text: &str) -> Option<Self> {
        let fields = text
            .split('|')
            .filter_map(|field| field.split_once('='))
            .collect::<HashMap<_, _>>();
        if fields.get("Active").copied() != Some("TRUE") {
            return None;
        }
        let number = |key: &str, default: f32| {
            fields
                .get(key)
                .and_then(|value| value.parse::<f32>().ok())
                .unwrap_or(default)
        };
        // A type the importer cannot evaluate is counted in the report and
        // its slider's own value used instead.
        let kind = CurveKind::parse(fields.get("Type").copied().unwrap_or("Flat"))?;
        let points = fields
            .get("Values")
            .map(|values| {
                values
                    .split(';')
                    .filter_map(|point| {
                        let (x, y) = point.split_once(':')?;
                        Some((x.parse::<f32>().ok()?, y.parse::<f32>().ok()?))
                    })
                    .collect()
            })
            .unwrap_or_default();
        Some(Self {
            kind,
            min: number("Min", 0.0),
            max: number("Max", 100.0),
            p: [
                number("P1", 0.0),
                number("P2", 0.0),
                number("P3", 0.0),
                number("P4", 0.0),
            ],
            points,
            real_values: fields.get("RV").copied() == Some("TRUE"),
            ends: fields.get("SE").copied() == Some("Y"),
        })
    }

    /// The curve's value at `t` in 0..1 of the effect, 0..1 of its range.
    fn normalized(&self, t: f32) -> f32 {
        // xLights' `ValueCurve`: P values in the slider's own units (`RV`)
        // where the curve type reads them so, else 0..100 of the range.
        let span = (self.max - self.min).abs().max(f32::EPSILON);
        let real = |p: f32| {
            if self.real_values {
                ((p - self.min) / span).clamp(0.0, 1.0)
            } else {
                (p / 100.0).clamp(0.0, 1.0)
            }
        };
        let [p1, p2, p3, p4] = self.p;
        let value = match self.kind {
            CurveKind::Flat => real(p1),
            CurveKind::Ramp | CurveKind::Random => real(p1) + (real(p2) - real(p1)) * t,
            CurveKind::RampUpDownHold => {
                let hold = p3 / 100.0;
                let (rise, fall) = (0.5 - 0.5 * hold, 0.5 + 0.5 * hold);
                if t < rise {
                    real(p1) + (real(p2) - real(p1)) * t / rise.max(f32::EPSILON)
                } else if t <= fall {
                    real(p2)
                } else {
                    real(p2) + (real(p1) - real(p2)) * (t - fall) / (1.0 - fall).max(f32::EPSILON)
                }
            }
            // xLights' generators: the shape's coefficient from P1 as written,
            // its offset from P2 in the slider's units.
            CurveKind::LogarithmicUp => {
                let a = if p1 == 0.0 { 0.04 } else { p1 / 25.0 };
                (real(p2) * 100.0 - 50.0) / 50.0 + (a + a * t).ln() + 1.0
            }
            CurveKind::LogarithmicDown => {
                let a = if p1 == 0.0 { 0.1 } else { p1 / 10.0 };
                (real(p2) * 100.0 - 50.0) / 50.0 + 1.5 - (a * t - 1.0).exp2()
            }
            CurveKind::ExponentialUp | CurveKind::ExponentialDown => {
                let a = if p1 == 0.0 { 0.1 } else { p1 / 10.0 };
                let rise = (a * t).exp_m1() / a.exp_m1();
                let shape = if self.kind == CurveKind::ExponentialUp {
                    rise
                } else {
                    1.0 - rise
                };
                (real(p2) * 100.0 - 50.0) / 50.0 + shape
            }
            CurveKind::RampUpDown => {
                if t < 0.5 {
                    real(p1) + (real(p2) - real(p1)) * t * 2.0
                } else {
                    real(p2) + (real(p3) - real(p2)) * (t - 0.5) * 2.0
                }
            }
            CurveKind::Saw => {
                let cycles = p3.max(1.0).floor();
                let phase = (t * cycles).fract();
                // Up to P2 at the middle of each tooth and back down to P1.
                let rise = 1.0 - (phase * 2.0 - 1.0).abs();
                real(p1) + (real(p2) - real(p1)) * rise
            }
            CurveKind::Square => {
                let cycles = p3.max(1.0);
                if (t * cycles).fract() < 0.5 {
                    real(p1)
                } else {
                    real(p2)
                }
            }
            CurveKind::Sine | CurveKind::AbsSine => {
                let r = t * std::f32::consts::TAU * (p3 / 10.0).max(0.1)
                    + p1 * std::f32::consts::TAU / 100.0;
                let center = (real(p4) * 100.0 - 50.0) / 50.0;
                if self.kind == CurveKind::AbsSine {
                    center + (r.sin() * p2.max(1.0) / 100.0).abs()
                } else {
                    center + r.sin() * p2.max(1.0) / 200.0 + 0.5
                }
            }
            CurveKind::ParabolicDown | CurveKind::ParabolicUp => {
                let a = if p1 == 0.0 { 1.0 } else { p1 };
                let a = if self.kind == CurveKind::ParabolicUp {
                    -a
                } else {
                    a
                };
                let y = a * (t - 0.5) * (t - 0.5) + real(p2);
                if self.ends {
                    real(p3) + y * (real(p4) - real(p3))
                } else {
                    y
                }
            }
            CurveKind::Custom => match self.points.as_slice() {
                [] => real(p1),
                points => {
                    let after = points.iter().position(|(x, _)| *x >= t);
                    match after {
                        Some(0) => points[0].1,
                        None => points[points.len() - 1].1,
                        Some(index) => {
                            let (x0, y0) = points[index - 1];
                            let (x1, y1) = points[index];
                            let span = (x1 - x0).max(f32::EPSILON);
                            y0 + (y1 - y0) * (t - x0) / span
                        }
                    }
                }
            },
        };
        value.clamp(0.0, 1.0)
    }

    /// The slider's value at `t` in 0..1 of the effect, in the slider's units.
    pub(crate) fn at(&self, t: f32) -> f32 {
        self.min + (self.max - self.min) * self.normalized(t)
    }
}

/// A palette color: one color, or a color curve across the effect.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum PaletteColor {
    Solid(Color),
    /// Stops `(position, color)` in 0..1; xLights' color curves.
    Curve(Vec<(f32, Color)>),
}

impl PaletteColor {
    /// The color at `t` in 0..1.
    pub(crate) fn at(&self, t: f32) -> Color {
        match self {
            Self::Solid(color) => *color,
            Self::Curve(stops) => {
                let after = stops.iter().position(|(x, _)| *x >= t);
                match after {
                    Some(0) | None if stops.is_empty() => Color::BLACK,
                    Some(0) => stops[0].1,
                    None => stops[stops.len() - 1].1,
                    Some(index) => {
                        let (x0, c0) = stops[index - 1];
                        let (x1, c1) = stops[index];
                        let k = ((t - x0) / (x1 - x0).max(f32::EPSILON)).clamp(0.0, 1.0);
                        Color {
                            red: c0.red + (c1.red - c0.red) * k,
                            green: c0.green + (c1.green - c0.green) * k,
                            blue: c0.blue + (c1.blue - c0.blue) * k,
                        }
                    }
                }
            }
        }
    }
}

/// `#rrggbb`.
pub(crate) fn hex_color(text: &str) -> Option<Color> {
    let hex = text.trim().strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let channel = |range: std::ops::Range<usize>| {
        u8::from_str_radix(hex.get(range)?, 16)
            .ok()
            .map(|value| f32::from(value) / 255.0)
    };
    Some(Color {
        red: channel(0..2)?,
        green: channel(2..4)?,
        blue: channel(4..6)?,
    })
}

/// A color button's value: `#rrggbb`, or a color curve
/// `Active=TRUE|Id=...|Values=x=0.000^c=#ff0000;x=1.000^c=#0000ff|`.
fn palette_color(text: &str) -> Option<PaletteColor> {
    if let Some(color) = hex_color(text) {
        return Some(PaletteColor::Solid(color));
    }
    let values = text
        .split('|')
        .find_map(|field| field.strip_prefix("Values="))?;
    let mut stops = values
        .split(';')
        .filter_map(|stop| {
            let (x, c) = stop.split_once('^')?;
            let x = x.strip_prefix("x=")?.parse::<f32>().ok()?;
            Some((x.clamp(0.0, 1.0), hex_color(c.strip_prefix("c=")?)?))
        })
        .collect::<Vec<_>>();
    stops.sort_by(|a, b| a.0.total_cmp(&b.0));
    match stops.as_slice() {
        [] => None,
        [(_, color)] => Some(PaletteColor::Solid(*color)),
        _ => Some(PaletteColor::Curve(stops)),
    }
}

/// An effect's color palette and color settings (`C_` keys).
pub(crate) struct Palette {
    /// The checked colors, in palette order.
    pub(crate) colors: Vec<PaletteColor>,
    settings: Settings,
}

impl Palette {
    /// The palette, or why one of its checked colors cannot be read.
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        let settings = Settings::parse(text);
        let mut colors = Vec::new();
        for index in 1..=8 {
            if !settings.bool(&format!("C_CHECKBOX_Palette{index}"), false) {
                continue;
            }
            let button = settings
                .text(&format!("C_BUTTON_Palette{index}"))
                .ok_or_else(|| format!("palette color {index} is checked but has no color"))?;
            colors.push(
                palette_color(button)
                    .ok_or_else(|| format!("palette color {index} is not a color: {button}"))?,
            );
        }
        Ok(Self { colors, settings })
    }

    /// A palette with no colors.
    pub(crate) fn empty() -> Self {
        Self {
            colors: Vec::new(),
            settings: Settings::default(),
        }
    }

    /// The palette's colors, or white when none is checked, as xLights
    /// draws an empty palette.
    pub(crate) fn colors_or_white(&self) -> Vec<PaletteColor> {
        if self.colors.is_empty() {
            vec![PaletteColor::Solid(WHITE)]
        } else {
            self.colors.clone()
        }
    }

    /// The types of the palette's active value curves the importer cannot
    /// evaluate.
    pub(crate) fn unknown_curves(&self) -> impl Iterator<Item = String> + '_ {
        self.settings.unknown_curves()
    }

    /// Brightness in 0..4 (xLights' slider goes to 400%).
    pub(crate) fn brightness(&self) -> f32 {
        self.settings.float("C_SLIDER_Brightness", 100.0) / 100.0
    }
}
