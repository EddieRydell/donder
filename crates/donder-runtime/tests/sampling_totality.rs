use donder_runtime::automation::{
    AutomationMapping, AutomationValue, automation_value_at_position,
};
use donder_runtime::dsl::Type;
use donder_runtime::sampling::{sample_curve, sample_gradient};
use donder_runtime::values::{Color, Curve, CurvePoint, Gradient, GradientStop};

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
    assert_eq!(sample_curve(&curve, f32::NAN), 0.0);

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
