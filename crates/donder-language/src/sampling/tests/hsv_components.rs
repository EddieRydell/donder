use crate::sampling::{color_hue, color_intensity, color_saturation, hsv};
use crate::values::Color;

fn color([red, green, blue]: [u8; 3]) -> Color {
    Color { red, green, blue }
}

#[test]
fn quantized_colors_round_trip_at_zero_and_full_turns() {
    for red in [0, 1, 2, 63, 64, 127, 128, 254, 255] {
        for green in [0, 1, 2, 63, 64, 127, 128, 254, 255] {
            for blue in [0, 1, 2, 63, 64, 127, 128, 254, 255] {
                let c = color([red, green, blue]);
                for shift in [0.0, -1.0, 1.0] {
                    assert_eq!(
                        hsv(
                            color_hue(c) + shift,
                            color_saturation(c),
                            color_intensity(c)
                        ),
                        c
                    );
                }
            }
        }
    }
}

#[test]
fn hsv_components_cover_every_byte_denominator_and_sector() {
    // Enumerate every possible max/chroma pair and every sector, including
    // ties. Compare against division independently of the implementation.
    for max in 0..=255u8 {
        for min in 0..=max {
            for middle in [min, min + (max - min) / 2, max] {
                for rgb in [
                    [max, middle, min],
                    [middle, max, min],
                    [min, max, middle],
                    [min, middle, max],
                    [middle, min, max],
                    [max, min, middle],
                ] {
                    let c = color(rgb);
                    let [r, g, b] = rgb.map(f32::from);
                    let chroma = f32::from(max - min);
                    let hue = if chroma == 0.0 {
                        0.0
                    } else {
                        let sector = if r == f32::from(max) {
                            (g - b) / chroma
                        } else if g == f32::from(max) {
                            (b - r) / chroma + 2.0
                        } else {
                            (r - g) / chroma + 4.0
                        };
                        let hue = sector / 6.0;
                        if hue < 0.0 { hue + 1.0 } else { hue }
                    };
                    let saturation = if max == 0 {
                        0.0
                    } else {
                        chroma / f32::from(max)
                    };
                    assert!((color_hue(c) - hue).abs() <= f32::EPSILON, "{c:?}");
                    assert!(
                        (color_saturation(c) - saturation).abs() <= f32::EPSILON,
                        "{c:?}"
                    );
                    assert!(
                        (color_intensity(c) - f32::from(max) / 255.0).abs() <= f32::EPSILON,
                        "{c:?}"
                    );
                    assert_eq!(
                        hsv(color_hue(c), color_saturation(c), color_intensity(c)),
                        c
                    );
                }
            }
        }
    }
}
