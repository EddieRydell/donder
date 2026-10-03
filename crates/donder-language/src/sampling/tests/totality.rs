use crate::automation::{AutomationMapping, AutomationValue, automation_value_at_position};
use crate::dsl::types::Type;
use crate::sampling::{curve_last_crossing, sample_curve, sample_gradient};
use crate::values::{Color, Curve, CurvePoint, Gradient, GradientStop};
use alloc::vec;

#[test]
fn single_point_resources_are_total_even_when_their_position_is_invalid() {
    let curve = Curve {
        points: vec![CurvePoint {
            position: f32::NAN,
            value: 0.75,
        }],
    };
    assert!(curve.validate().is_err());
    assert_eq!(sample_curve(&curve, 0.25), 0.75);
    assert!(sample_curve(&curve, f32::NAN).is_nan());

    let color = Color {
        red: 1,
        green: 2,
        blue: 3,
    };
    let gradient = Gradient {
        stops: vec![GradientStop {
            position: f32::NAN,
            color,
        }],
    };
    assert!(gradient.validate().is_err());
    assert_eq!(sample_gradient(&gradient, 0.25), color);
    assert_eq!(sample_gradient(&gradient, f32::NAN), Color::BLACK);
}

#[test]
fn empty_gradient_is_valid_and_samples_black() {
    let gradient = Gradient { stops: vec![] };
    assert!(gradient.validate().is_ok());
    assert_eq!(sample_gradient(&gradient, 0.5), Color::BLACK);
}

#[test]
fn missing_curve_samples_propagate_but_endpoints_are_held() {
    let empty = Curve { points: vec![] };
    assert!(sample_curve(&empty, 0.0).is_nan());
    let curve = Curve {
        points: vec![
            CurvePoint {
                position: 0.25,
                value: -2.0,
            },
            CurvePoint {
                position: 0.75,
                value: 3.0,
            },
        ],
    };
    assert_eq!(sample_curve(&curve, f32::NEG_INFINITY), -2.0);
    assert_eq!(sample_curve(&curve, f32::INFINITY), 3.0);
    assert_eq!(sample_curve(&curve, 0.5), 0.5);
    assert!(sample_curve(&curve, f32::NAN).is_nan());
}

#[test]
fn latest_crossing_counts_plateau_arrival_once_and_not_its_departure() {
    let curve = Curve {
        points: vec![
            CurvePoint {
                position: 0.0,
                value: 0.0,
            },
            CurvePoint {
                position: 0.25,
                value: 1.0,
            },
            CurvePoint {
                position: 0.5,
                value: 1.0,
            },
            CurvePoint {
                position: 0.75,
                value: 1.0,
            },
            CurvePoint {
                position: 1.0,
                value: 0.0,
            },
        ],
    };
    assert!(curve_last_crossing(&curve, 1.0, 0.24).is_nan());
    for position in [0.25, 0.5, 0.75, 1.0, f32::INFINITY] {
        assert_eq!(curve_last_crossing(&curve, 1.0, position), 0.25);
    }
    assert_eq!(curve_last_crossing(&curve, 0.5, 0.8), 0.125);
    assert_eq!(curve_last_crossing(&curve, 0.5, 0.875), 0.875);
    assert_eq!(curve_last_crossing(&curve, 0.0, 0.5), 0.0);
    assert_eq!(curve_last_crossing(&curve, 0.0, 1.0), 1.0);
    assert!(curve_last_crossing(&curve, f32::NAN, 1.0).is_nan());
    assert!(curve_last_crossing(&curve, 0.5, f32::NAN).is_nan());
}

#[test]
fn latest_crossing_does_not_invent_values_inside_a_discontinuous_step() {
    let curve = Curve {
        points: vec![
            CurvePoint {
                position: 0.0,
                value: 0.0,
            },
            CurvePoint {
                position: 0.5,
                value: 0.0,
            },
            CurvePoint {
                position: 0.5,
                value: 1.0,
            },
            CurvePoint {
                position: 1.0,
                value: 1.0,
            },
        ],
    };
    assert!(curve_last_crossing(&curve, 0.5, 1.0).is_nan());
    assert_eq!(curve_last_crossing(&curve, 1.0, 1.0), 0.5);
}

#[test]
fn descending_automation_endpoints_remain_well_typed_and_total() {
    let curve = Curve {
        points: vec![
            CurvePoint {
                position: 0.0,
                value: 0.0,
            },
            CurvePoint {
                position: 1.0,
                value: 1.0,
            },
        ],
    };
    let float = AutomationMapping::Float { min: 1.0, max: 0.0 };
    let int = AutomationMapping::Int { min: 10, max: 0 };
    let window = AutomationMapping::Curve { min: 1.0, max: 0.0 };
    assert!(float.accepts_type(&Type::Float));
    assert!(int.accepts_type(&Type::Int));
    assert!(window.accepts_type(&Type::Curve));
    assert!(matches!(
        automation_value_at_position(&curve, &float, 0.25),
        Some(AutomationValue::Float(value)) if value == 0.75
    ));
    assert!(matches!(
        automation_value_at_position(&curve, &int, 0.25),
        Some(AutomationValue::Int(8))
    ));
}
