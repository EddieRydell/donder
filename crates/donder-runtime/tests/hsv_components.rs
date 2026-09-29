use donder_runtime::sampling::{color_hue, color_intensity, color_saturation, hsv};
use donder_runtime::values::Color;

fn color([red, green, blue]: [u8; 3]) -> Color {
    Color { red, green, blue }
}

#[test]
fn normalized_hsv_components_cover_sectors_and_achromatic_colors() {
    for (rgb, hue, saturation, value) in [
        ([255, 0, 0], 0.0, 1.0, 1.0),
        ([255, 255, 0], 1.0 / 6.0, 1.0, 1.0),
        ([0, 255, 0], 2.0 / 6.0, 1.0, 1.0),
        ([0, 255, 255], 3.0 / 6.0, 1.0, 1.0),
        ([0, 0, 255], 4.0 / 6.0, 1.0, 1.0),
        ([255, 0, 255], 5.0 / 6.0, 1.0, 1.0),
        ([128, 64, 96], 11.0 / 12.0, 0.5, 128.0 / 255.0),
        ([128, 128, 128], 0.0, 0.0, 128.0 / 255.0),
        ([255, 255, 255], 0.0, 0.0, 1.0),
        ([0, 0, 0], 0.0, 0.0, 0.0),
    ] {
        let c = color(rgb);
        assert!((color_hue(c) - hue).abs() < 1e-6, "{c:?}");
        assert!((color_saturation(c) - saturation).abs() < 1e-6, "{c:?}");
        assert!((color_intensity(c) - value).abs() < 1e-6, "{c:?}");
    }
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
