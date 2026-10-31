//! Vixen's preview shapes, each laid out as its `Layout()` places its
//! pixels when the preview loads: in preview units, x to the right and y
//! down, at zoom 1. Positions are Vixen's own formulas without the whole-unit
//! rounding Vixen applies for drawing, so evenly spaced lights stay evenly
//! spaced. Every shape's code names the Vixen code it follows, at
//! `VixenLights/Vixen` commit `e111ae41963abfa48b70f884445950e94e432e02`
//! (`src/Vixen.Modules/Preview/VixenPreview/Shapes`).

mod arch;
mod cane;
mod custom;
mod custom_prop;
mod ellipse;
mod icicle;
mod line;
mod mega_tree;
mod multi_string;
mod pixel_grid;
mod poly_line;
mod rectangle;
mod single;
mod star;
mod star_burst;
mod triangle;

use super::contract;
use crate::xml::Element;
use glam::DVec2;

/// One of a shape's preview pixels.
#[derive(Clone, Debug)]
pub(crate) struct ShapePixel {
    /// The element it lights, by its node id; none for a pixel linked to
    /// nothing.
    pub(crate) node: Option<String>,
    /// Where Vixen draws it, in preview units.
    pub(crate) position: DVec2,
    /// The ellipse of [`Laid::ellipses`] the pixel is on, and how far along
    /// it in degrees, when the shape places it on one.
    pub(crate) arc: Option<(usize, f64)>,
}

/// The points `center + u cos φ + v sin φ`, in preview units.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Ellipse2 {
    pub(crate) center: DVec2,
    pub(crate) u: DVec2,
    pub(crate) v: DVec2,
}

/// A shape's pixels, in the order of its `Pixels` list, which for a shape
/// of strings is each string's pixels in turn.
#[derive(Clone, Debug, Default)]
pub(crate) struct Laid {
    pub(crate) pixels: Vec<ShapePixel>,
    pub(crate) ellipses: Vec<Ellipse2>,
}

pub(crate) use crate::report::ShapeSkip;

/// The node of each of a shape's own `Pixels`, in order. Vixen:
/// `PreviewLightBaseShape._pixels`, `PreviewPixel.NodeId`. A high-precision
/// pixel is refused: `SetPixelZoomRotate` puts it at its saved `Location`
/// over the shape's layout.
pub(crate) fn pixel_nodes(shape: &Element) -> Result<Vec<Option<String>>, ShapeSkip> {
    let Some(pixels) = contract::field(shape, "Pixels") else {
        return Ok(Vec::new());
    };
    pixels
        .children("PreviewPixel")
        .map(|pixel| {
            if contract::boolean(pixel, "IsHighPrecision", false)? {
                return Err(ShapeSkip::Setting("high-precision pixel locations"));
            }
            contract::guid(pixel, "NodeId").map_err(ShapeSkip::from)
        })
        .collect()
}

/// The shape's child strings, in order. Vixen: `PreviewLightBaseShape._strings`.
pub(crate) fn strings(shape: &Element) -> Vec<&Element> {
    contract::field(shape, "_strings")
        .map(|strings| strings.children.iter().collect())
        .unwrap_or_default()
}

/// Refuses a rotated shape: Vixen rotates about a centre it computes from
/// the shape's laid-out bounds while the shape is still loading.
pub(crate) fn unrotated(shape: &Element) -> Result<(), ShapeSkip> {
    match contract::int(shape, "RotationAngle", 0)? {
        0 => Ok(()),
        _ => Err(ShapeSkip::Setting("rotation")),
    }
}

/// The shape a `DisplayItem`'s `Shape` element holds, laid out.
pub(crate) fn layout(shape: &Element) -> Result<Laid, ShapeSkip> {
    let kind = contract::type_name(shape).unwrap_or_default();
    match kind {
        "PreviewLine" => line::layout(shape),
        "PreviewPolyLine" => poly_line::layout(shape),
        "PreviewArch" => arch::layout(shape),
        "PreviewEllipse" => ellipse::layout(shape),
        "PreviewCane" => cane::layout(shape),
        "PreviewRectangle" => rectangle::layout(shape),
        "PreviewMegaTree" => mega_tree::layout(shape),
        "PreviewPixelGrid" => pixel_grid::layout(shape),
        "PreviewIcicle" => icicle::layout(shape),
        "PreviewMultiString" => multi_string::layout(shape),
        "PreviewStar" => star::layout(shape),
        "PreviewStarBurst" => star_burst::layout(shape),
        "PreviewTriangle" => triangle::layout(shape),
        "PreviewSingle" => single::layout(shape),
        "PreviewCustom" => custom::layout(shape),
        "PreviewCustomProp" => custom_prop::layout(shape),
        other => Err(ShapeSkip::Type(other.to_string())),
    }
}
