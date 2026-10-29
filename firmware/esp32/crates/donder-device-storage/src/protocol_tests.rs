//! The editor's commands decode with the firmware's embedded JSON parser.
use donder_device_protocol::{Command, Mode, Speed, SpeedFrameTiming};

#[test]
fn clock_command_decodes_with_the_embedded_parser() {
    let payload = br#"{"syncClock":{"bootId":1,"clockId":2,"localMicros":4294967300,"masterMicros":100000,"ratePpb":-100,"validForMicros":15000000}}"#;
    let (command, used) = serde_json_core::from_slice::<Command>(payload)
        .expect("Clock command must decode with the firmware parser");
    assert_eq!(used, payload.len());
    assert_eq!(
        command,
        Command::SyncClock {
            boot_id: 1,
            clock_id: 2,
            local_micros: 4294967300,
            master_micros: 100000,
            rate_ppb: -100,
            valid_for_micros: 15000000
        }
    );
}

#[test]
fn scheduled_and_cancel_commands_decode_with_the_embedded_parser() {
    let payload = br#"{"schedule":{"bootId":1,"clockId":2,"commandId":3,"atMicros":4294967400,"mode":"playing","positionMicros":1000000,"speed":{"showMicrosPerSecond":500000,"frameTiming":"constant"},"looping":false,"showId":123,"showBytes":11860}}"#;
    let (command, used) = serde_json_core::from_slice::<Command>(payload).unwrap();
    assert_eq!(used, payload.len());
    assert_eq!(
        command,
        Command::Schedule {
            boot_id: 1,
            clock_id: 2,
            command_id: 3,
            at_micros: 4294967400,
            mode: Mode::Playing,
            position_micros: 1000000,
            speed: Speed {
                show_micros_per_second: 500000,
                frame_timing: SpeedFrameTiming::Constant
            },
            looping: false,
            show_id: 123,
            show_bytes: 11860
        }
    );
    let (command, _) =
        serde_json_core::from_slice::<Command>(br#"{"cancel":{"commandId":3}}"#).unwrap();
    assert_eq!(command, Command::Cancel { command_id: 3 });
}

#[test]
fn a_stopped_speed_has_no_rate() {
    let speed = |show_micros_per_second| Speed {
        show_micros_per_second,
        frame_timing: SpeedFrameTiming::Scaled,
    };
    assert_eq!(speed(0).rate(), None);
    assert_eq!(
        speed(1_000_000).rate(),
        Some(donder_runtime::PlaybackRate::NORMAL)
    );
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
