//! Where a model sits in the xLights layout: the transform from the model's
//! own screen coordinates to layout coordinates, computed in single
//! precision as xLights computes it. X is right, Y up and Z toward the
//! viewer, as in Donder.

use crate::report::SkipReason;
use crate::xlights::read::Reader;
use glam::{Mat4, Vec3, Vec4};

/// The model's size in its own coordinates, which two- and three-point
/// placements stretch between their points. xLights: `RenderWi`, set by
/// each model's `SetRenderSize`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RenderSize {
    pub(crate) width: f32,
}

#[derive(Clone, Debug)]
pub(crate) enum Placement {
    Boxed {
        position: Vec3,
        scale: Vec3,
        /// Degrees about X, Y and Z, as written.
        rotation: Vec3,
    },
    TwoPoint {
        position: Vec3,
        offset: Vec3,
    },
    ThreePoint {
        position: Vec3,
        offset: Vec3,
        /// Degrees about the line, `RotateX`.
        tilt: f32,
        /// Stretches the model's height, unless the model lays out its own
        /// height.
        height: Option<f32>,
        /// Icicles only.
        shear: Option<f32>,
    },
    /// A transform the model computes itself, as a Poly Line's does from
    /// its points.
    Matrix(Mat4),
}

fn vector(reader: &Reader, names: [&str; 3], default: f32) -> Result<Vec3, SkipReason> {
    Ok(Vec3::new(
        reader.as_float(names[0], default)?,
        reader.as_float(names[1], default)?,
        reader.as_float(names[2], default)?,
    ))
}

/// xLights: `DeserializeModelScreenLocationAttributes`.
fn world(reader: &Reader) -> Result<Vec3, SkipReason> {
    vector(reader, ["WorldPosX", "WorldPosY", "WorldPosZ"], 0.0)
}

/// xLights: `DeserializeTwoPointScreenLocationAttributes`.
fn second_point(reader: &Reader) -> Result<Vec3, SkipReason> {
    vector(reader, ["X2", "Y2", "Z2"], 0.0)
}

/// xLights: `DeserializeModelScreenLocationAttributes`, then
/// `BoxedScreenLocation::Init`, which replaces a negative scale with 1.
pub(crate) fn boxed(reader: &Reader) -> Result<Placement, SkipReason> {
    let scale = vector(reader, ["ScaleX", "ScaleY", "ScaleZ"], 1.0)?;
    Ok(Placement::Boxed {
        position: world(reader)?,
        scale: Vec3::select(scale.cmplt(Vec3::ZERO), Vec3::ONE, scale),
        rotation: vector(reader, ["RotateX", "RotateY", "RotateZ"], 0.0)?,
    })
}

pub(crate) fn two_point(reader: &Reader) -> Result<Placement, SkipReason> {
    Ok(Placement::TwoPoint {
        position: world(reader)?,
        offset: second_point(reader)?,
    })
}

/// xLights: `DeserializeThreePointScreenLocationAttributes`. `own_height`
/// for the models that lay out their own height
/// (`SetModelHandleHeight(true)`: Arches and Candy Canes); `shear` for
/// Icicles (`SetSupportsShear(true)`).
pub(crate) fn three_point(
    reader: &Reader,
    own_height: bool,
    shear: bool,
) -> Result<Placement, SkipReason> {
    Ok(Placement::ThreePoint {
        position: world(reader)?,
        offset: second_point(reader)?,
        tilt: reader.as_float("RotateX", 0.0)?,
        height: (!own_height)
            .then(|| reader.as_float("Height", 1.0))
            .transpose()?,
        shear: shear.then(|| reader.as_float("Shear", 0.0)).transpose()?,
    })
}

/// xLights: `glm::rotate(angle, axis)`.
fn rotate(radians: f32, axis: Vec3) -> Mat4 {
    let (s, c) = radians.sin_cos();
    let axis = axis.normalize();
    let temp = axis * (1.0 - c);
    Mat4::from_cols(
        Vec4::new(
            c + temp.x * axis.x,
            temp.x * axis.y + s * axis.z,
            temp.x * axis.z - s * axis.y,
            0.0,
        ),
        Vec4::new(
            temp.y * axis.x - s * axis.z,
            c + temp.y * axis.y,
            temp.y * axis.z + s * axis.x,
            0.0,
        ),
        Vec4::new(
            temp.z * axis.x + s * axis.y,
            temp.z * axis.y - s * axis.x,
            c + temp.z * axis.z,
            0.0,
        ),
        Vec4::W,
    )
}

/// xLights: `VectorMath::rotationMatrixFromXAxisToVector2`.
fn x_axis_to_line(origin: Vec3, point: Vec3) -> Mat4 {
    let a = point - origin;
    if origin.y != point.y || origin.z != point.z {
        let length = a.length();
        let angle = if length == 0.0 {
            0.0
        } else {
            (a.x / length).acos()
        };
        rotate(angle, Vec3::new(0.0, -a.z, a.y))
    } else if point.x < origin.x {
        rotate(180f32.to_radians(), Vec3::Z)
    } else {
        Mat4::IDENTITY
    }
}

/// xLights: `VectorMath::rotationMatrixFromXAxisToVector`, which snaps a
/// line within about a degree of the X axis onto it.
pub(crate) fn x_axis_to_vector(a: Vec3) -> Mat4 {
    let ax = a.x / a.length();
    let angle = ax.acos();
    if ax > 0.9999 || angle == 0.0 {
        Mat4::IDENTITY
    } else if ax < -0.9999 || (angle - std::f32::consts::PI).abs() < 0.0001 {
        rotate(180f32.to_radians(), Vec3::Y)
    } else {
        rotate(angle, Vec3::new(0.0, -a.z, a.y))
    }
}

/// The placement's second point, moved off the first when they are the
/// same, as xLights does so the line has a length.
fn second(position: Vec3, offset: Vec3) -> Vec3 {
    let x = if offset == Vec3::ZERO {
        0.001
    } else {
        offset.x
    };
    Vec3::new(x + position.x, offset.y + position.y, offset.z + position.z)
}

impl Placement {
    /// xLights: each screen location's `PrepareToDraw(true, ...)` and
    /// `TranslatePoint`.
    pub(crate) fn transform(&self, size: RenderSize) -> Mat4 {
        match *self {
            // xLights: BoxedScreenLocation::TranslatePoint, with the
            // rotation `ModelScreenLocation::SetRotation` builds from the
            // degrees as written.
            Self::Boxed {
                position,
                scale,
                rotation,
            } => {
                let rotation = rotate(rotation.z.to_radians(), Vec3::Z)
                    * rotate(rotation.y.to_radians(), Vec3::Y)
                    * rotate(rotation.x.to_radians(), Vec3::X);
                Mat4::from_translation(position) * rotation * Mat4::from_scale(scale)
            }
            // xLights: TwoPointScreenLocation::PrepareToDraw.
            Self::TwoPoint { position, offset } => {
                let point = second(position, offset);
                let scale = (point - position).length() / size.width;
                Mat4::from_translation(position)
                    * x_axis_to_line(position, point)
                    * Mat4::from_scale(Vec3::splat(scale))
            }
            // xLights: ThreePointScreenLocation::PrepareToDraw.
            Self::ThreePoint {
                position,
                offset,
                tilt,
                height,
                shear,
            } => {
                let point = second(position, offset);
                let swapped = offset.x < 0.0;
                let (from, to) = if swapped {
                    (point, position)
                } else {
                    (position, point)
                };
                let a = to - from;
                let scale = a.length() / size.width;
                let mut rotation = x_axis_to_vector(a);
                if swapped {
                    rotation *= rotate(180f32.to_radians(), Vec3::Y);
                }
                let stretch =
                    Mat4::from_scale(Vec3::new(scale, scale * height.unwrap_or(1.0), scale));
                // xLights: glm::shearY, x' = x + shear * y.
                let mut shearing = Mat4::IDENTITY;
                shearing.y_axis.x = shear.unwrap_or(0.0);
                Mat4::from_translation(position)
                    * rotation
                    * rotate(tilt.to_radians(), Vec3::X)
                    * shearing
                    * stretch
            }
            Self::Matrix(matrix) => matrix,
        }
    }
}
