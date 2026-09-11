use crate::dto::Point3Meters;
use dawn_language::values::Point3;

pub(crate) fn point3_meters(point: Point3) -> Point3Meters {
    Point3Meters {
        x_meters: point.x.as_meters_f32(),
        y_meters: point.y.as_meters_f32(),
        z_meters: point.z.as_meters_f32(),
    }
}
