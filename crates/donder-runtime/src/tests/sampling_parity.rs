use crate::dsl::{BoundParams, DslBindCache, Type, Value};
use crate::sampling::{multiply_colors, sample_curve, sample_gradient};
use crate::values::{Color, Curve, CurvePoint, Gradient, GradientStop};
use alloc::vec;

#[test]
fn color_multiply_rounds_to_nearest_for_every_channel_pair() {
    for a in 0..=255u16 {
        for b in 0..=255u16 {
            let color = |x| Color {
                red: x,
                green: x,
                blue: x,
            };
            let expected = ((f64::from(a) * f64::from(b)) / 255.0).round() as u8;
            assert_eq!(
                multiply_colors(color(a as u8), color(b as u8)),
                color(expected)
            );
        }
    }
}

#[test]
fn gradient_parameter_and_direct_sampling_agree_at_steps_and_boundaries() {
    let gradient = Gradient {
        stops: [
            (
                0.0,
                Color {
                    red: 0,
                    green: 0,
                    blue: 0,
                },
            ),
            (
                0.5,
                Color {
                    red: 255,
                    green: 0,
                    blue: 0,
                },
            ),
            (
                0.5,
                Color {
                    red: 0,
                    green: 255,
                    blue: 0,
                },
            ),
            (
                1.0,
                Color {
                    red: 0,
                    green: 0,
                    blue: 255,
                },
            ),
        ]
        .into_iter()
        .map(|(position, color)| GradientStop { position, color })
        .collect(),
    };
    let params = BoundParams::bind_values(
        &[Type::Gradient],
        vec![Value::Gradient(gradient.clone().into())],
        &mut DslBindCache::default(),
    )
    .unwrap();
    for position in [-1.0, 0.0, 0.25, 0.499, 0.5, 0.501, 0.75, 1.0, 2.0] {
        assert_eq!(
            params.sample_gradient(0, position).unwrap(),
            sample_gradient(&gradient, position)
        );
    }
    assert_eq!(sample_gradient(&gradient, 0.5), gradient.stops[2].color);
    assert_eq!(sample_gradient(&gradient, f32::NAN), Color::BLACK);
    assert_eq!(
        sample_gradient(&Gradient { stops: vec![] }, 0.5),
        Color::BLACK
    );
    assert!(
        sample_curve(
            &Curve {
                points: vec![
                    CurvePoint {
                        position: 0.0,
                        value: 1.0
                    },
                    CurvePoint {
                        position: 1.0,
                        value: 2.0
                    },
                ],
            },
            f32::NAN,
        )
        .is_nan()
    );
}
