use dawn_runtime::patch::{PixelEncoding, PreparedPatch, PreparedPixelRoute};
use dawn_runtime::values::Color;

#[test]
fn rgbw_extraction_channel_order_and_lookup_are_applied_exactly() {
    let colors: Vec<_> = (0..=255)
        .map(|red| Color {
            red,
            green: 255 - red,
            blue: red / 2,
        })
        .collect();
    let lookup = std::array::from_fn(|index| 255 - index as u8);
    let patch = PreparedPatch {
        routes: vec![
            PreparedPixelRoute {
                pixels: 0..256,
                frame: 0,
                start_slot: 2,
                encoding: PixelEncoding::Rgbw {
                    order: [3, 1, 0, 2],
                },
                lookup: Some(0),
            },
            PreparedPixelRoute {
                pixels: 0..256,
                frame: 1,
                start_slot: 0,
                encoding: PixelEncoding::Rgb { order: [1, 0, 2] },
                lookup: None,
            },
        ]
        .into_boxed_slice(),
        lookups: vec![lookup].into_boxed_slice(),
    };
    let mut buffers = vec![vec![77; 1028], vec![77; 768]];
    patch.evaluate(&colors, &mut buffers).unwrap();
    assert_eq!(&buffers[0][..2], [0, 0]);
    assert_eq!(&buffers[0][1026..], [0, 0]);
    for (index, color) in colors.iter().enumerate() {
        let white = color.red.min(color.green).min(color.blue);
        assert_eq!(
            &buffers[0][2 + index * 4..6 + index * 4],
            [
                255 - white,
                255 - (color.green - white),
                255 - (color.red - white),
                255 - (color.blue - white)
            ]
        );
        assert_eq!(
            &buffers[1][index * 3..index * 3 + 3],
            [color.green, color.red, color.blue]
        );
    }
}
