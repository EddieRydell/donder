//! QuinLED Dig-Quad v2/v3 with the QuinLED classic ESP32 module.
//! Flush UART0 and disable its async interrupt and application diagnostics before
//! these outputs take ownership of the pins; retain the UART clock for ROM calls.

use esp_hal::{
    gpio::NoPin,
    i2s::parallel::TxEightBits,
    peripherals::{GPIO1, GPIO3, GPIO4, GPIO16},
};

pub const OUTPUT_DESCRIPTION: &str = "gpio16,3,1,4";
// Conservative commissioning limit until device power configuration exists.
pub const MAX_CHANNEL_VALUE: u8 = 25;

pub fn output_pins(
    led1: GPIO16<'static>,
    led2: GPIO3<'static>,
    led3: GPIO1<'static>,
    led4: GPIO4<'static>,
) -> TxEightBits<'static> {
    TxEightBits::new(led1, led2, led3, led4, NoPin, NoPin, NoPin, NoPin)
}
