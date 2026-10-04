use donder_language::values::{Curve, CurvePoint, CurveValidationError};

#[test]
fn unsorted_curves_are_rejected() {
    let curve = Curve {
        points: vec![
            CurvePoint {
                position: 0.5,
                value: 0.0,
            },
            CurvePoint {
                position: 0.25,
                value: 1.0,
            },
        ],
    };
    assert_eq!(
        curve.validate(),
        Err(CurveValidationError::PositionsNotStrictlyIncreasing)
    );
}
