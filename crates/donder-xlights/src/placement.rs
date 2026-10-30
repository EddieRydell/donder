//! Where a model sits in the xLights layout: the transform from its own
//! coordinates to layout coordinates. X is right, Y up and Z toward the
//! viewer, as in Donder.

use crate::models::Spec;
use crate::report::SkipReason;
use glam::{DAffine3, DMat3, DQuat, DVec3};

pub(crate) enum Placement {
    /// Scaled per axis, rotated about X, then Y, then Z, then moved.
    Boxed {
        position: DVec3,
        scale: DVec3,
        degrees: DVec3,
    },
    /// Model x from 0 to `width` runs from `start` to `start + offset`.
    TwoPoint {
        start: DVec3,
        offset: DVec3,
        width: f64,
    },
    /// Two points as above, the model tilted `tilt` degrees about the line
    /// between them, its y sheared by `shear` x and stretched by `height`.
    ThreePoint {
        start: DVec3,
        offset: DVec3,
        width: f64,
        tilt: f64,
        height: f64,
        shear: f64,
    },
    /// Model coordinates scaled per axis, then moved.
    Points { position: DVec3, scale: DVec3 },
}

fn point(spec: &Spec, x: &str, y: &str, z: &str, default: f64) -> Result<DVec3, SkipReason> {
    Ok(DVec3::new(
        spec.float(x, default)?,
        spec.float(y, default)?,
        spec.float(z, default)?,
    ))
}

pub(crate) fn position(spec: &Spec) -> Result<DVec3, SkipReason> {
    point(spec, "WorldPosX", "WorldPosY", "WorldPosZ", 0.0)
}

pub(crate) fn boxed(spec: &Spec) -> Result<Placement, SkipReason> {
    Ok(Placement::Boxed {
        position: position(spec)?,
        scale: point(spec, "ScaleX", "ScaleY", "ScaleZ", 1.0)?,
        degrees: point(spec, "RotateX", "RotateY", "RotateZ", 0.0)?,
    })
}

/// The line from the model's first point to its second, which must be two
/// places for the model to have a direction and a length.
fn offset(spec: &Spec) -> Result<DVec3, SkipReason> {
    let offset = point(spec, "X2", "Y2", "Z2", 0.0)?;
    if offset == DVec3::ZERO {
        return Err(SkipReason::Unplaceable("its two end points are the same"));
    }
    Ok(offset)
}

pub(crate) fn two_point(spec: &Spec, width: f64) -> Result<Placement, SkipReason> {
    Ok(Placement::TwoPoint {
        start: position(spec)?,
        offset: offset(spec)?,
        width,
    })
}

/// `stretch` when the placement's height stretches the model, rather than
/// the model laying out its own height.
pub(crate) fn three_point(spec: &Spec, width: f64, stretch: bool) -> Result<Placement, SkipReason> {
    Ok(Placement::ThreePoint {
        start: position(spec)?,
        offset: offset(spec)?,
        width,
        tilt: spec.float("RotateX", 0.0)?,
        height: if stretch {
            spec.float("Height", 1.0)?
        } else {
            1.0
        },
        shear: spec.float("Shear", 0.0)?,
    })
}

pub(crate) fn points(spec: &Spec) -> Result<Placement, SkipReason> {
    Ok(Placement::Points {
        position: position(spec)?,
        scale: point(spec, "ScaleX", "ScaleY", "ScaleZ", 1.0)?,
    })
}

/// The shortest turn from +X onto `direction`; a line pointing back along X
/// turns about Z.
fn x_onto(direction: DVec3) -> DQuat {
    let direction = direction.normalize();
    if direction.abs_diff_eq(DVec3::NEG_X, 1e-12) {
        DQuat::from_rotation_z(std::f64::consts::PI)
    } else {
        DQuat::from_rotation_arc(DVec3::X, direction)
    }
}

impl Placement {
    pub(crate) fn transform(&self) -> DAffine3 {
        match *self {
            Self::Boxed {
                position,
                scale,
                degrees,
            } => {
                let rotation = DQuat::from_rotation_z(degrees.z.to_radians())
                    * DQuat::from_rotation_y(degrees.y.to_radians())
                    * DQuat::from_rotation_x(degrees.x.to_radians());
                DAffine3::from_scale_rotation_translation(scale, rotation, position)
            }
            Self::TwoPoint {
                start,
                offset,
                width,
            } => {
                let scale = offset.length() / width;
                DAffine3::from_scale_rotation_translation(
                    DVec3::splat(scale),
                    x_onto(offset),
                    start,
                )
            }
            Self::ThreePoint {
                start,
                offset,
                width,
                tilt,
                height,
                shear,
            } => {
                let scale = offset.length() / width;
                // A line drawn right to left keeps the model upright.
                let rotation = if offset.x < 0.0 {
                    x_onto(-offset) * DQuat::from_rotation_y(std::f64::consts::PI)
                } else {
                    x_onto(offset)
                };
                let sheared = DMat3::from_cols(DVec3::new(1.0, shear, 0.0), DVec3::Y, DVec3::Z);
                let linear = DMat3::from_quat(rotation)
                    * DMat3::from_rotation_x(tilt.to_radians())
                    * sheared
                    * DMat3::from_diagonal(DVec3::new(scale, scale * height, scale));
                DAffine3::from_mat3_translation(linear, start)
            }
            Self::Points { position, scale } => {
                DAffine3::from_scale_rotation_translation(scale, DQuat::IDENTITY, position)
            }
        }
    }
}
