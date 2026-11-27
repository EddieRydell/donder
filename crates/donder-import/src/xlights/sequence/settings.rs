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
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum CurveKind {
    Flat,
    Ramp,
    RampUp,
    RampDown,
    RampUpDown,
    Saw,
    Square,
    Sine,
    AbsSine,
    Parabolic,
    Logarithmic,
    Exponential,
    Custom,
    Random,
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
        let kind = match fields.get("Type").copied().unwrap_or("Flat") {
            "Flat" => CurveKind::Flat,
            "Ramp" => CurveKind::Ramp,
            "Ramp Up" => CurveKind::RampUp,
            "Ramp Down" => CurveKind::RampDown,
            "Ramp Up/Down" => CurveKind::RampUpDown,
            "Saw Tooth" => CurveKind::Saw,
            "Square" => CurveKind::Square,
            "Sine" => CurveKind::Sine,
            "Abs Sine" => CurveKind::AbsSine,
            "Parabolic Down" | "Parabolic Up" => CurveKind::Parabolic,
            "Logarithmic Up" | "Logarithmic Down" => CurveKind::Logarithmic,
            "Exponential Up" | "Exponential Down" => CurveKind::Exponential,
            "Custom" => CurveKind::Custom,
            "Random" => CurveKind::Random,
            _ => CurveKind::Flat,
        };
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
            CurveKind::Ramp
            | CurveKind::Logarithmic
            | CurveKind::Exponential
            | CurveKind::Random => real(p1) + (real(p2) - real(p1)) * t,
            CurveKind::RampUp => t,
            CurveKind::RampDown => 1.0 - t,
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
            CurveKind::Parabolic => {
                let a = if p1 == 0.0 { 1.0 } else { p1 };
                a * (t - 0.5) * (t - 0.5) + real(p2)
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
    pub(crate) fn parse(text: &str) -> Self {
        let settings = Settings::parse(text);
        let colors = (1..=8)
            .filter(|index| settings.bool(&format!("C_CHECKBOX_Palette{index}"), false))
            .filter_map(|index| palette_color(settings.text(&format!("C_BUTTON_Palette{index}"))?))
            .collect();
        Self { colors, settings }
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

    /// Brightness in 0..4 (xLights' slider goes to 400%).
    pub(crate) fn brightness(&self) -> f32 {
        self.settings.float("C_SLIDER_Brightness", 100.0) / 100.0
    }
}
