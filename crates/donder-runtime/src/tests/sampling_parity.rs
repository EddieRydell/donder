use crate::sampling::multiply_colors;
use donder_runtime_types::Color;

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
