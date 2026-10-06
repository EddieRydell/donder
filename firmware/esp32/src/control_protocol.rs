//! Firmware control commands decoded by the embedded JSON parser.
use crate::transport;

#[derive(serde::Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Command {
    SyncClock {
        boot_id: u32,
        clock_id: u32,
        local_micros: u64,
        master_micros: u64,
        rate_ppb: i32,
        valid_for_micros: u32,
    },
    Schedule {
        boot_id: u32,
        clock_id: u32,
        command_id: u32,
        at_micros: u64,
        mode: transport::Mode,
        position_micros: u32,
        speed: Speed,
        looping: bool,
        archive_crc: u32,
        archive_bytes: u32,
    },
    Cancel {
        command_id: u32,
    },
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Speed {
    pub show_micros_per_second: u32,
    pub frame_timing: FrameTiming,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FrameTiming {
    Scaled,
    Constant,
}

impl Speed {
    pub fn rate(&self) -> Option<donder_runtime::PlaybackRate> {
        Some(donder_runtime::PlaybackRate::new(
            core::num::NonZeroU32::new(self.show_micros_per_second)?,
            match self.frame_timing {
                FrameTiming::Scaled => donder_runtime::FrameTiming::Scaled,
                FrameTiming::Constant => donder_runtime::FrameTiming::Constant,
            },
        ))
    }
}

impl<'de> serde::Deserialize<'de> for transport::Mode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl serde::de::Visitor<'_> for Visitor {
            type Value = transport::Mode;
            fn expecting(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                f.write_str("playing, paused, or stopped")
            }
            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
                match value {
                    "playing" => Ok(transport::Mode::Playing),
                    "paused" => Ok(transport::Mode::Paused),
                    "stopped" => Ok(transport::Mode::Stopped),
                    _ => Err(E::custom("Invalid transport mode")),
                }
            }
        }
        deserializer.deserialize_str(Visitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clock_command_decodes_with_the_embedded_parser() {
        let payload = br#"{"syncClock":{"bootId":1,"clockId":2,"localMicros":4294967300,"masterMicros":100000,"ratePpb":-100,"validForMicros":15000000}}"#;
        let (command, used) = serde_json_core::from_slice::<Command>(payload)
            .expect("Clock command must decode with the firmware parser");
        assert_eq!(used, payload.len());
        assert!(matches!(
            command,
            Command::SyncClock {
                boot_id: 1,
                clock_id: 2,
                local_micros: 4294967300,
                master_micros: 100000,
                rate_ppb: -100,
                valid_for_micros: 15000000
            }
        ));
    }
    #[test]
    fn scheduled_and_cancel_commands_decode_with_the_embedded_parser() {
        let payload = br#"{"schedule":{"bootId":1,"clockId":2,"commandId":3,"atMicros":4294967400,"mode":"playing","positionMicros":1000000,"speed":{"showMicrosPerSecond":500000,"frameTiming":"constant"},"looping":false,"archiveCrc":123,"archiveBytes":11860}}"#;
        let (command, used) = serde_json_core::from_slice::<Command>(payload).unwrap();
        assert_eq!(used, payload.len());
        assert!(matches!(
            command,
            Command::Schedule {
                boot_id: 1,
                clock_id: 2,
                command_id: 3,
                at_micros: 4294967400,
                mode: transport::Mode::Playing,
                position_micros: 1000000,
                speed: Speed {
                    show_micros_per_second: 500000,
                    frame_timing: FrameTiming::Constant
                },
                looping: false,
                archive_crc: 123,
                archive_bytes: 11860
            }
        ));
        let (command, _) =
            serde_json_core::from_slice::<Command>(br#"{"cancel":{"commandId":3}}"#).unwrap();
        assert!(matches!(command, Command::Cancel { command_id: 3 }));
    }
    #[test]
    fn malformed_commands_are_rejected_by_the_embedded_parser() {
        for payload in [
            &br#"{"cancel":{"commandId":3,"extra":true}}"#[..],
            &br#"{"cancel":{}}"#[..],
            &br#"{"unknown":{"commandId":3}}"#[..],
            &br#"{"syncClock":{"bootId":1}}"#[..],
        ] {
            assert!(serde_json_core::from_slice::<Command>(payload).is_err());
        }
    }
}
