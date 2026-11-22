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
mod net;
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

/// The node of each of a shape's own `Pixels`, in order, refusing a pixel
/// that keeps a high-precision location: Vixen then places it at that saved
/// location rather than where the shape's layout puts it. Vixen:
/// `PreviewLightBaseShape._pixels`, `PreviewPixel.NodeId`,
/// `PreviewLightBaseShape.SetPixelZoomRotate`, `PreviewPixel.Point`.
pub(crate) fn pixel_nodes(shape: &Element) -> Result<Vec<Option<String>>, ShapeSkip> {
    let Some(pixels) = contract::field(shape, "Pixels") else {
        return Ok(Vec::new());
    };
    pixels
        .children("PreviewPixel")
        .map(|pixel| {
            if contract::boolean(pixel, "IsHighPrecision", false)? {
                return Err(ShapeSkip::Setting("high-precision lights"));
            }
            contract::guid(pixel, "NodeId").map_err(ShapeSkip::from)
        })
        .collect()
}

/// A preview point as a position. Vixen: `PreviewPoint`.
fn position((x, y): (i32, i32)) -> DVec2 {
    DVec2::new(f64::from(x), f64::from(y))
}

/// The point a field holds, as a position.
fn point(shape: &Element, name: &str) -> Result<DVec2, ShapeSkip> {
    Ok(position(contract::point(shape, name)?))
}

/// The shape's child strings, in order. Vixen: `PreviewLightBaseShape._strings`.
pub(crate) fn strings(shape: &Element) -> Vec<&Element> {
    contract::field(shape, "_strings")
        .map(|strings| strings.children.iter().collect())
        .unwrap_or_default()
}

/// The shape's rotation in degrees clockwise on screen; Vixen's setter
/// ignores an angle outside ±360. Vixen: `PreviewBaseShape.RotationAngle`.
pub(crate) fn rotation(shape: &Element) -> Result<f64, ShapeSkip> {
    Ok(match contract::int(shape, "RotationAngle", 0)? {
        angle @ -360..=360 => f64::from(angle),
        _ => 0.0,
    })
}

/// `point` turned `angle` degrees clockwise on screen about `center`, in
/// the order WPF's `RotateTransform` computes it. Vixen:
/// `PreviewCustomProp.Rotate`; WPF `Matrix.RotateAt`, `Matrix.Transform`.
pub(crate) fn rotated(point: DVec2, angle: f64, center: DVec2) -> DVec2 {
    let radians = (angle % 360.0) * (std::f64::consts::PI / 180.0);
    let sin = radians.sin();
    let cos = radians.cos();
    let offset_x = center.x * (1.0 - cos) + center.y * sin;
    let offset_y = center.y * (1.0 - cos) - center.x * sin;
    DVec2::new(
        point.x * cos + (point.y * -sin + offset_x),
        point.y * cos + (point.x * sin + offset_y),
    )
}

/// `laid` turned by the shape's rotation about the centre of its lights'
/// bounds. Vixen computes that centre while the shape is still loading, so
/// a rotated shape lands close to, not exactly at, where Vixen draws it.
/// A shape's child strings keep the shape's rotation only.
fn rotate(mut laid: Laid, angle: f64) -> Laid {
    if angle == 0.0 || laid.pixels.is_empty() {
        return laid;
    }
    let (min, max) = laid.pixels.iter().fold(
        (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)),
        |(min, max), pixel| (min.min(pixel.position), max.max(pixel.position)),
    );
    let center = (min + max) / 2.0;
    for pixel in &mut laid.pixels {
        pixel.position = rotated(pixel.position, angle, center);
    }
    let origin = rotated(DVec2::ZERO, angle, DVec2::ZERO);
    let turn = |vector: DVec2| rotated(vector, angle, DVec2::ZERO) - origin;
    for ellipse in &mut laid.ellipses {
        ellipse.center = rotated(ellipse.center, angle, center);
        ellipse.u = turn(ellipse.u);
        ellipse.v = turn(ellipse.v);
    }
    laid
}

/// The shape a `DisplayItem`'s `Shape` element holds, laid out and rotated.
pub(crate) fn layout(shape: &Element) -> Result<Laid, ShapeSkip> {
    let kind = contract::type_name(shape).unwrap_or_default();
    if kind == "PreviewCustomProp" {
        // A custom prop turns about its own lights' saved bounds.
        return custom_prop::layout(shape);
    }
    let angle = rotation(shape)?;
    let laid = match kind {
        "PreviewLine" => line::layout(shape),
        "PreviewPolyLine" => poly_line::layout(shape),
        "PreviewArch" => arch::layout(shape),
        "PreviewEllipse" => ellipse::layout(shape),
        "PreviewCane" => cane::layout(shape),
        "PreviewRectangle" => rectangle::layout(shape),
        "PreviewMegaTree" => mega_tree::layout(shape),
        "PreviewPixelGrid" => pixel_grid::layout(shape),
        "PreviewNet" => net::layout(shape),
        "PreviewIcicle" => icicle::layout(shape),
        "PreviewMultiString" => multi_string::layout(shape),
        "PreviewStar" => star::layout(shape),
        "PreviewStarBurst" => star_burst::layout(shape),
        "PreviewTriangle" => triangle::layout(shape),
        "PreviewSingle" => single::layout(shape),
        "PreviewCustom" => custom::layout(shape),
        other => Err(ShapeSkip::Type(other.to_string())),
    }?;
    Ok(rotate(laid, angle))
}
