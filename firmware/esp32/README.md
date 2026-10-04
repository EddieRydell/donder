# Donder ESP32 firmware

This separate Cargo workspace builds the controller loader for the classic ESP32:
Wi-Fi upload and control, flash storage, and four-output WS281x playback. It
consumes `donder-runtime` and `donder-language` with default features off;
parsing, imports, target resolution and output selection stay on the host. The
local `crates/donder-device-storage` crate owns LittleFS credentials and the two
show slots. How the controller behaves is described in
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

- `loader`: persistent credentials and Wi-Fi upload.
- `i2s-output`: adds continuous four-output WS281x playback on the second core,
  on the reference-board pins. This is the bundled desktop image.
- `dig-quad`: the same, with QuinLED Dig-Quad pins and a 25/255 brightness cap.

From the repository root:

```powershell
pnpm firmware:build                      # bundled image, also refreshes apps/desktop/assets/firmware
pnpm firmware:build --board dig-quad     # Dig-Quad image (also overwrites the bundled image)
pnpm firmware:cargo build --release --features i2s-output --bin loader --locked
```

`firmware:build` refuses an image that would overlap the data partitions.
Flashing directly must use `partitions.csv`, or the Donder data layout is lost.
19,200 baud is the rate the desktop installer also uses reliably:

```powershell
cd firmware/esp32
espflash flash --port COM4 --baud 19200 --chip esp32 --non-interactive --flash-size 4mb --flash-mode dio --flash-freq 40mhz --partition-table partitions.csv --target-app-partition factory target/xtensa-esp32-none-elf/release/loader
```

## Uploading and measuring

Export a selected fragment with checksums, then upload and verify it with
`upload.py` (run through `uvx --from esptool`, which supplies pyserial):

```powershell
cargo run -p donder-elaboration --example export_sequence -- examples/starter firmware/esp32/target/loaded-sequence.donderseq
cd firmware/esp32
uvx --from esptool python upload.py target/loaded-sequence.donderseq --port COM4 --ssid YOUR_SSID
uvx --from esptool python upload.py target/loaded-sequence.donderseq --checksums target/loaded-sequence.donderseq.checksums --elf target/xtensa-esp32-none-elf/release/loader --windows-profile YOUR_PROFILE --uploads 3 --exercise-rejections --repeat 1 --monitor-seconds 75 --log target/i2s-playback.txt
```

- The Wi-Fi password prompt is not echoed. On English Windows,
  `--windows-profile` reads a saved network profile in memory. Never write
  credentials or tokens to logs.
- Claiming that frames are verified requires `--checksums`; `--elf` records the
  image hash.
- Uploads stay stopped, so start playback through the desktop or
  `POST /transport/play` first.
- `upload.py` rejects incomplete records, checksum mismatches and corrupt serial
  data. Never repair a capture by deleting bytes.
- Serial monitoring needs UART0 free, so it is unavailable on the Dig-Quad.

During playback the loader prints a `PLAYBACK` line per window: missed frames and
average and maximum evaluation, encoding, DMA-wait and total frame times. To
measure a runtime change, upload a representative show such as
`examples/stanford_room` and compare those lines. Keep captures under `target/`
and summarize durable results in [performance](../../docs/performance.md).

## Implementation notes

- Flash reads and writes use aligned 256-byte scratch buffers and the SDK's
  low-level flash routines.
- A flash mutation first waits for the renderer to acknowledge its IRAM
  checkpoint, then takes the cross-core critical section and parks that core.
  Parking the core in hardware without the checkpoint can freeze an outstanding
  cache fill. Refreshing the show mapping flushes both caches while the other
  core is parked, because a flush can disturb that core's fill even when it does
  not read the show mapping.
- Upload blackout waits between completed DMA transfers in an interrupt-free IRAM
  loop.
- Keep large flash buffers off the core-0 startup and network stack. Show
  restoration and upload persistence share it with filesystem and SDK calls.
- The runtime `atomic` feature is enabled only for output builds, so the active
  sequence and its workspace can cross cores.
- `rwtext_hook.x` places the interpreter and per-lane helpers in instruction RAM
  (see [performance](../../docs/performance.md#esp32-memory-placement)).

## Validation

From the repository root:

```powershell
pnpm storage:test
cargo fmt --manifest-path firmware/esp32/Cargo.toml --check
pnpm firmware:cargo clippy --release --features i2s-output --bin loader --locked -- -D warnings
pnpm firmware:cargo clippy --release --features dig-quad --bin loader --locked -- -D warnings
```

`pnpm storage:test` is part of `pnpm check`. It uses the host toolchain and
`DONDER_HOST_LIBCLANG_PATH` (see the root
[development setup](../../README.md#development)) and builds in
`target/storage-host`. These checks do not flash a board. Report source checks,
device checksums, deadlines, DMA completion and physical LED output separately.
