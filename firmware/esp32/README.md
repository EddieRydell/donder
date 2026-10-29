# Donder ESP32 firmware

This separate Cargo workspace builds the controller loader for the classic ESP32:
Wi-Fi upload and control, flash storage, and four-output WS281x playback. It
consumes `donder-runtime` and `donder-runtime-types` with default features off;
parsing, imports, target resolution and output selection stay on the host. The
local `crates/donder-device-storage` crate owns the LittleFS configuration record
(name, station network and claim token) and the two show slots. How the controller behaves is described in
[ESP32 controllers](../../docs/esp32_loading.md).

## Toolchain

Install the ESP Rust toolchain with [espup](https://github.com/esp-rs/espup) and
install `espflash`. Set two machine-local environment variables, using the
locations espup reports:

- `DONDER_ESP_LIBCLANG_PATH`: ESP's libclang file or its directory.
- `DONDER_ESP_TOOLCHAIN_BIN`: the directory containing `xtensa-esp32-elf-gcc`.

`pnpm firmware:build` and `pnpm firmware:cargo` set PATH, LIBCLANG_PATH and
CARGO_TARGET_DIR for their child processes only, building into this directory's
`target/`. Never run host builds with `+esp`.

The esp-hal SDK crates are patched together to upstream revision
`0eb3e53b4a2e555d2136ce9dc83e18c6692b9673`, because the published radio beta
targets an older HAL. Keep them on one source, build with `--locked`, and remove
the patches together once a compatible release exists and the board checks are
repeated.

## Features and builds

Every build has access point or station networking, mDNS discovery, claiming,
Wi-Fi upload and continuous four-output WS281x playback on the second core, on
the reference-board pins. The only feature is:

- `dig-quad`: QuinLED Dig-Quad pins and a 25/255 brightness cap.

From the repository root:

```powershell
pnpm firmware:build                      # reference-board image in target/firmware/donder-esp32.bin
pnpm firmware:build --board dig-quad     # Dig-Quad image, same path
pnpm firmware:cargo build --release --bin loader --locked
```

Flashing directly must use `partitions.csv`, or the Donder data layout is lost.
The desktop does not flash controllers; flash them with espflash:

```powershell
cd firmware/esp32
espflash flash --port COM4 --baud 460800 --chip esp32 --non-interactive --flash-size 4mb --flash-mode dio --flash-freq 40mhz --partition-table partitions.csv --target-app-partition factory target/xtensa-esp32-none-elf/release/loader
```

## Uploading and measuring

The editor uploads each controller's show when Play is pressed (see
[ESP32 loading](../../docs/esp32_loading.md)). Uploads stay stopped, so start
playback through the editor or `POST /transport/play`. To check a build's
frames against the host, export a fragment with checksums:

```powershell
cargo run -p donder-elaboration --example export_show -- examples/starter firmware/esp32/target/loaded.dondershow
```

`PUT /show` loads the exported show, and `POST /frame` with a frame's tick
as a 4-byte little-endian body replies `FRAME tick crc32 micros`, the CRC of
the frame's output bytes and its evaluation time; each tick and CRC must match
a row of the `.checksums` file. Never write tokens to logs.

To measure a runtime change, upload a representative show such as
`examples/stanford_room` and compare its `POST /frame` times. Keep captures
under `target/` and summarize durable results in
[performance](../../docs/performance.md).

## Implementation notes

- Flash reads and writes use aligned 256-byte scratch buffers and the SDK's
  low-level flash routines.
- A flash mutation first waits for the renderer to acknowledge its IRAM
  checkpoint, then takes the cross-core critical section and parks that core.
  Parking the core in hardware without the checkpoint can freeze an outstanding
  cache fill. While the other core is parked, its cache is disabled for the
  flash operation, then flushed and enabled again.
- Upload blackout waits between completed DMA transfers in an interrupt-free IRAM
  loop.
- Keep large flash buffers off the core-0 startup and network stack. Show
  restoration and upload persistence share it with filesystem and SDK calls.
- The runtime `atomic` feature lets the active show and its workspace cross
  cores.
- The runtime's `iram` feature places the interpreter and graph evaluation in
  instruction RAM, and `pnpm firmware:build` fails if any of that code is left
  in flash (see
  [performance](../../docs/performance.md#esp32-memory-placement)).

## Validation

From the repository root:

```powershell
pnpm storage:test
cargo fmt --manifest-path firmware/esp32/Cargo.toml --check
pnpm firmware:cargo clippy --release --bin loader --locked -- -D warnings
pnpm firmware:cargo clippy --release --features dig-quad --bin loader --locked -- -D warnings
```

`pnpm storage:test` is part of `pnpm check`. It uses the host toolchain and
`DONDER_HOST_LIBCLANG_PATH` (see the root
[development setup](../../README.md#development)) and builds in
`target/storage-host`. These checks do not flash a board. Report source checks,
device checksums, deadlines, DMA completion and physical LED output separately.
