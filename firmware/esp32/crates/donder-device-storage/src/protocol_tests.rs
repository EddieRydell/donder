//! The editor's commands decode with the firmware's embedded JSON parser.
use core::num::NonZeroU32;
use donder_device_protocol::{Action, Command};
use donder_runtime::{FrameTiming, PlaybackRate};

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
    let payload = br#"{"schedule":{"bootId":1,"clockId":2,"commandId":3,"atMicros":4294967400,"action":"play","positionMicros":1000000,"rate":{"showMicrosPerSecond":500000,"frameTiming":"constant"},"looping":false,"showId":123,"showBytes":11860}}"#;
    let (command, used) = serde_json_core::from_slice::<Command>(payload).unwrap();
    assert_eq!(used, payload.len());
    assert_eq!(
        command,
        Command::Schedule {
            boot_id: 1,
            clock_id: 2,
            command_id: 3,
            at_micros: 4294967400,
            action: Action::Play,
            position_micros: 1000000,
            rate: PlaybackRate::new(NonZeroU32::new(500000).unwrap(), FrameTiming::Constant),
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
fn malformed_commands_are_rejected_by_the_embedded_parser() {
    for payload in [
        &br#"{"cancel":{"commandId":3,"extra":true}}"#[..],
        &br#"{"cancel":{}}"#[..],
        &br#"{"unknown":{"commandId":3}}"#[..],
        &br#"{"syncClock":{"bootId":1}}"#[..],
        // A stopped playback has no rate.
        &br#"{"schedule":{"bootId":1,"clockId":2,"commandId":3,"atMicros":0,"action":"play","positionMicros":0,"rate":{"showMicrosPerSecond":0,"frameTiming":"scaled"},"looping":false,"showId":1,"showBytes":1}}"#[..],
    ] {
        assert!(serde_json_core::from_slice::<Command>(payload).is_err());
    }
}
