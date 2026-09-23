# Prepared sequence loading on ESP32

The ESP32 loader runs `donder-runtime`; it is not a second interpreter. The host
loads and validates source, elaborates generators and targets, selects controller
ports, and writes a `.donderseq` archive. The device validates and decodes that
prepared representation before replacing its active immutable sequence.

## Install from Donder

The supported user path is the desktop controller editor. Build the bundled
controller image from the repository root when firmware changes:

```powershell
./firmware/esp32/build-image.ps1
```

The script builds the output-enabled loader, packages
`target/firmware/donder-esp32.bin`, and refreshes the committed image and SHA-256
under `apps/desktop/assets/firmware`. Do not edit those generated assets by hand.
Installation writes the application image but deliberately does not include the
persistent Donder data partition.

After installation, provision the board's 2.4 GHz Wi-Fi credentials from Donder,
export a compiled sequence for that controller's output ports, and upload it.
The development uploader provides the same flow from `firmware/esp32`:

```powershell
python upload.py ../../target/show.donderseq --port COM4 --ssid YOUR_SSID
```

The password prompt is not echoed. On English Windows,
`--windows-profile PROFILE` can read a saved personal-network profile in memory.
Credentials and upload tokens must never be written to logs or evidence files.

## Transport and replacement

On boot the loader reads credentials and an optional prepared archive from the
reserved LittleFS partition. If credentials are absent it prints a provisioning
token on UART. Sequence upload uses an ephemeral token printed on UART and the
following HTTP surface:

- `GET /capabilities`
- `PUT /sequence`
- `POST /frame`
- with `i2s-output`: `GET /transport` and `POST /transport/play`, `/pause`, or
  `/stop`

An upload is decoded into a candidate first. Invalid authentication, header,
version, size, checksum, or prepared content rejects the candidate and preserves
the active sequence. Concurrent upload attempts are rejected. Persistence occurs
only after validation; interruption must not manufacture an accepted archive.

The current admission limits are 32 KiB of archive payload, 1,600 pixels, 128
graph nodes, and 96 KiB of estimated workspace. They are conservative policy,
not an OOM proof. Loader-only builds reserve archive decode headroom. The I2S
build instead derives admission from remaining heap while preserving the active
sequence during replacement.

## Build and verify

Export a representative selected fragment from the repository root:

```powershell
cargo run -p donder-elaboration --example export_sequence -- examples/starter firmware/esp32/target/loaded-sequence.donderseq
```

Build and flash from `firmware/esp32`:

```powershell
. ./export-esp.ps1
cargo +esp build --release --features i2s-output --bin loader --locked
espflash flash --port COM4 --baud 19200 --chip esp32 --non-interactive --flash-size 4mb --flash-mode dio --flash-freq 40mhz --partition-table partitions.csv --target-app-partition factory target/xtensa-esp32-none-elf/release/loader
```

Verify uploads, rejection behavior, frame checksums, and continuous playback:

```powershell
uvx --from esptool python upload.py target/loaded-sequence.donderseq --checksums target/loaded-sequence.donderseq.checksums --elf target/xtensa-esp32-none-elf/release/loader --windows-profile YOUR_PROFILE --uploads 3 --exercise-rejections --repeat 1 --monitor-seconds 75 --log target/i2s-playback.txt
```

Supplying `--checksums` is required for a frame-verification claim; an ordinary
upload only proves transport and admission. Supplying `--elf` records the exact
image hash. Captures belong under ignored output until reviewed. The promotion
policy and retained baseline are in [Performance and hardware evidence](performance.md).

## Parallel WS281x output

The `i2s-output` feature drives up to 200 RGB pixels on each of GPIO13, GPIO18,
GPIO21, and GPIO25. I2S1 runs in 8-bit parallel mode at 2.4 MHz. Each WS281x bit
uses three samples (`100` for zero, `110` for one), producing a 1.25-us bit cell.
Two complete 15,120-byte DMA buffers allow the CPU to evaluate and encode the
next frame while the peripheral transmits the current frame; there are no
mid-frame CPU refills.

Wi-Fi and HTTP run on core 0. Sequence evaluation, parallel encoding, and DMA run
on core 1. The runtime's `atomic` feature is enabled only for this multicore
build so immutable active sequence state and its workspace can cross the core
boundary.

Device timing and DMA completion do not prove physical output. A hardware
acceptance result must separately state whether LEDs or an oscilloscope were
connected and what was measured.
