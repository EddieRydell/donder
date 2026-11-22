//! The profile's curve and color gradient libraries, the named curves and
//! gradients its effects pick from. Vixen: `CurveLibraryStaticData` and
//! `ColorGradientLibraryStaticData`, each a dictionary from name to value.

use super::contract::{self, FieldError};
use crate::xml::Element;
use donder_runtime_types::{Color, Curve, CurvePoint, Gradient, GradientSpace, GradientStop};

/// The curve library's module, whose type and instance ids are the same.
pub(crate) const CURVE_LIBRARY: &str = "4e258de2-7a75-4f0f-aa43-c8182e7f3400";
/// The color gradient library's module.
pub(crate) const GRADIENT_LIBRARY: &str = "64f4ab26-3ed4-49a3-a004-23656ed0424a";

/// Vixen's curves and gradient colors run in percent.
const PERCENT: f64 = 100.0;

/// Each entry of a library dictionary: its key and value.
fn entries(data: &Element) -> impl Iterator<Item = (&str, &Element)> {
    contract::field(data, "_library")
        .into_iter()
        .flat_map(|library| &library.children)
        .filter_map(|entry| Some((entry.child("Key")?.text.trim(), entry.child("Value")?)))
}

/// Each library curve, normalized: positions and values from Vixen's
/// percent to 0..1, in position order. Vixen: `Curve.Points`, a ZedGraph
/// `PointPairList`.
pub(crate) fn curves(data: &Element) -> Result<Vec<(String, Curve)>, FieldError> {
    entries(data)
        .map(|(name, value)| {
            let mut points = contract::required(value, "Points")?
                .children("PointPair")
                .map(|pair| {
                    let at = |field| {
                        contract::double(pair, field, 0.0)
                            .map(|percent| (percent / PERCENT).clamp(0.0, 1.0) as f32)
                    };
                    Ok(CurvePoint {
                        position: at("X")?,
                        value: at("Y")?,
                    })
                })
                .collect::<Result<Vec<_>, FieldError>>()?;
            points.sort_by(|a, b| a.position.total_cmp(&b.position));
            Ok((name.to_string(), Curve { points }))
        })
        .collect()
}

/// Each library gradient: its color stops, by position, in sRGB from
/// Vixen's CIE XYZ (D65, in percent). Vixen: `ColorGradient._colors`.
/// Its alpha stops and the focus between stops have no Donder equivalent.
pub(crate) fn gradients(data: &Element) -> Result<Vec<(String, Gradient)>, FieldError> {
    entries(data)
        .map(|(name, value)| {
            let mut stops = contract::field(value, "_colors")
                .into_iter()
                .flat_map(|colors| colors.children("ColorPoint"))
                .map(|point| {
                    let color = contract::required(point, "_color")?;
                    let xyz = [
                        contract::double(color, "_x", 0.0)?,
                        contract::double(color, "_y", 0.0)?,
                        contract::double(color, "_z", 0.0)?,
                    ];
                    Ok(GradientStop {
                        position: (contract::double(point, "_position", 0.0)?).clamp(0.0, 1.0)
                            as f32,
                        color: srgb(xyz),
                    })
                })
                .collect::<Result<Vec<_>, FieldError>>()?;
            stops.sort_by(|a, b| a.position.total_cmp(&b.position));
            Ok((
                name.to_string(),
                Gradient {
                    space: GradientSpace::LinearRgb,
                    stops,
                },
            ))
        })
        .collect()
}

/// A CIE XYZ color in percent, D65, as sRGB.
fn srgb([x, y, z]: [f64; 3]) -> Color {
    let [x, y, z] = [x, y, z].map(|part| part / PERCENT);
    let linear = [
        3.2406 * x - 1.5372 * y - 0.4986 * z,
        -0.9689 * x + 1.8758 * y + 0.0415 * z,
        0.0557 * x - 0.2040 * y + 1.0570 * z,
    ];
    let [red, green, blue] = linear.map(|part| {
        let part = part.clamp(0.0, 1.0);
        let encoded = if part <= 0.003_130_8 {
            12.92 * part
        } else {
            1.055 * part.powf(1.0 / 2.4) - 0.055
        };
        encoded as f32
    });
    Color { red, green, blue }
}
