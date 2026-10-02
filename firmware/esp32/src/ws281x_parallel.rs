extern crate alloc;

/// Encode GRB output bytes for the classic ESP32's 8-bit I2S1 parallel mode.
/// Every WS281x bit becomes `100` or `110` at a 2.4 MHz sample rate.
pub fn encode(outputs: &[impl AsRef<[u8]>], pixels: usize, buffer: &mut [u8], brightness: u8) {
    assert!(!outputs.is_empty() && outputs.len() <= 8);
    let data_samples = pixels * 24 * 3;
    assert!(buffer.len() >= data_samples && buffer.len().is_multiple_of(4));
    buffer[data_samples..].fill(0);

    let active_lanes = ((1_u16 << outputs.len()) - 1) as u8;
    let mut sample = 0;
    for byte_index in 0..pixels * 3 {
        let mut channels = [0_u8; 8];
        for (lane, output) in outputs.iter().enumerate() {
            let value = output.as_ref().get(byte_index).copied().unwrap_or(0);
            channels[lane] = (u16::from(value) * u16::from(brightness) / u16::from(u8::MAX)) as u8;
        }
        for bit in (0..8).rev() {
            let mut high_lanes = 0;
            for (lane, value) in channels[..outputs.len()].iter().enumerate() {
                high_lanes |= ((value >> bit) & 1) << lane;
            }

            // I2S1 emits each four-byte group as [2, 3, 0, 1]. XOR 2 is
            // the inverse mapping and lets us write samples in wire order.
            buffer[sample ^ 2] = active_lanes;
            buffer[(sample + 1) ^ 2] = high_lanes;
            buffer[(sample + 2) ^ 2] = 0;
            sample += 3;
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec;

    use super::encode;

    fn wire_sample(buffer: &[u8], index: usize) -> u8 {
        buffer[index ^ 2]
    }

    #[test]
    fn bits_are_constant_three_sample_cells_in_msb_first_lane_order() {
        let outputs = vec![vec![0b1010_0101], vec![0b0101_1010]];
        let mut buffer = vec![0xff; 72];
        encode(&outputs, 1, &mut buffer, u8::MAX);

        let expected_high = [0b01, 0b10, 0b01, 0b10, 0b10, 0b01, 0b10, 0b01];
        for (bit, expected) in expected_high.into_iter().enumerate() {
            let offset = bit * 3;
            assert_eq!(wire_sample(&buffer, offset), 0b11);
            assert_eq!(wire_sample(&buffer, offset + 1), expected);
            assert_eq!(wire_sample(&buffer, offset + 2), 0);
        }
        assert!((24..72).all(
            |sample| wire_sample(&buffer, sample) == 0b11 && sample % 3 == 0
                || wire_sample(&buffer, sample) == 0
        ));
    }

    #[test]
    fn shorter_outputs_are_zero_padded_and_reset_samples_stay_low() {
        let outputs = vec![vec![], vec![0xff; 3]];
        let mut buffer = vec![0xff; 80];
        encode(&outputs, 1, &mut buffer, u8::MAX);

        for bit in 0..24 {
            let offset = bit * 3;
            assert_eq!(wire_sample(&buffer, offset), 0b11);
            assert_eq!(wire_sample(&buffer, offset + 1), 0b10);
            assert_eq!(wire_sample(&buffer, offset + 2), 0);
        }
        assert!((72..80).all(|sample| wire_sample(&buffer, sample) == 0));
    }
}
