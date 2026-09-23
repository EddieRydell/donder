use donder_language::sampling::sample_curve;
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

#[test]
fn validated_curves_sample_finitely_at_every_generated_position() {
    for seed in 0..128u64 {
        let points = (0..9)
            .map(|index| CurvePoint {
                position: index as f32 / 8.0,
                value: pseudo_random(seed.wrapping_add(index as u64)),
            })
            .collect::<Vec<_>>();
        let curve = Curve { points };
        assert!(curve.validate().is_ok());
        for sample in 0..257 {
            let value = sample_curve(&curve, sample as f32 / 256.0);
            assert!(value.is_finite());
        }
    }
}

fn pseudo_random(seed: u64) -> f32 {
    let mixed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
    (mixed >> 11) as f32 / ((u64::MAX >> 11) as f32)
}
