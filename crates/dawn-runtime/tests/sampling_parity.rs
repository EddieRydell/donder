use dawn_runtime::dsl::{BoundParams, Identifier, ParamDecl, Type, Value};
use dawn_runtime::sampling::{multiply_colors, sample_gradient};
use dawn_runtime::values::{Color, Gradient, GradientStop};

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
fn gradient_parameter_and_native_sampling_agree_at_steps_and_boundaries() {
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
    let params = BoundParams::bind(
        &[ParamDecl {
            fixed: false,
            name: Identifier::new("gradient".into()).unwrap(),
            ty: Type::Gradient,
            default: Some(Value::Gradient(gradient.clone().into())),
        }],
        core::iter::empty(),
    )
    .unwrap();
    for position in [-1.0, 0.0, 0.25, 0.499, 0.5, 0.501, 0.75, 1.0, 2.0] {
        assert_eq!(
            params.sample_gradient(0, position).unwrap(),
            sample_gradient(&gradient, position).unwrap()
        );
    }
    assert_eq!(
        sample_gradient(&gradient, 0.5),
        Some(gradient.stops[2].color)
    );
}
