# Prepared sequence loading on ESP32

The ESP32 loader runs `donder-runtime`; it is not a second interpreter. The host
loads and validates source, prepares targets and effects, selects controller
ports, and writes a `.donderseq` archive. The device validates and decodes that
prepared representation before replacing its active immutable sequence.

## Install from Donder

The supported user path is the desktop controller editor. Build the bundled
controller image from the repository root when firmware changes:

```powershell
pnpm firmware:build
```

The script builds the output-enabled loader, packages
`target/firmware/donder-esp32.bin`, and refreshes the committed image and SHA-256
under `apps/desktop/assets/firmware`. Do not edit those generated assets by hand.
Installation writes the application image but deliberately does not include the
persistent credential and show partitions.

After installation, provision the board's 2.4 GHz Wi-Fi credentials from Donder,
export a compiled sequence for that controller's output ports, and upload it.
Uploads and restored sequences remain stopped and black until **Play** is requested.
The development uploader provides the same flow from `firmware/esp32`:

```powershell
python upload.py ../../target/show.donderseq --port COM4 --ssid YOUR_SSID
```

The password prompt is not echoed. On English Windows,
`--windows-profile PROFILE` can read a saved personal-network profile in memory.
Credentials and upload tokens must never be written to logs or evidence files.

## Transport and replacement

On boot the loader reads credentials and an optional prepared archive from the
reserved partitions. Credentials use LittleFS; archives use two contiguous
128 KiB flash slots in a separate 256 KiB partition. Provisioning returns a device
token on UART and stores it with the Wi-Fi credentials. Uploads and control
requests use that token until reprovisioning
replaces it. The following HTTP surface is available:

- `GET /capabilities`
- `PUT /sequence`
- `POST /frame`
- with `i2s-output`: `GET /transport` and `POST /transport/play`, `/pause`, or
  `/stop`
- with `i2s-output`: authenticated `GET /clock` and JSON `POST /control` for
  clock synchronization, scheduled transport, and cancellation

Uploads first ask the renderer to transmit black and wait between completed DMA
transfers in an interrupt-free IRAM loop. The renderer remains suspended while
flash is accessed. After authentication and size checks, uploads release the
previous decoded show and workspace before streaming to the inactive flash slot
through a 1 KiB buffer. Output stays black during replacement. The candidate is
validated once and decoded directly from memory-mapped flash. An upload or
admission failure leaves no loaded playback and reports the error; the
previous committed archive remains saved. Concurrent uploads are rejected.
The new slot is committed only after admission; interrupted staging cannot
supersede the previous saved archive. Reboot restores the newest committed slot
and leaves playback stopped. Explicit saved-data erase clears both partitions.
Prepared-content admission also checks sample and operator bytecode register references, operand spans, constants, jump targets,
parameter-read types, and reachable return paths before the VM can execute an
upload.
Admission rejects signal reads outside an operator's connected input table and
return instructions that do not match the program's effect or operator context.

The current admission limits are 96 KiB of archive payload, 1,600 pixels, 128
graph nodes, and 96 KiB of estimated workspace. They are conservative policy,
not an OOM proof. Workspace admission derives from remaining heap. Upload bytes
stay in flash; two decoded shows are never retained simultaneously. Payload size
alone does not bound decoded memory, so larger compiler-produced shows still
require representative device admission measurements.

Flash reads and writes use bounded, aligned 256-byte scratch buffers with the
SDK's low-level flash routines. Flash mutations acquire the cross-core critical
section before parking the already-suspended rendering core. Hardware parking
alone can freeze an outstanding cache fill; the renderer must first acknowledge
its IRAM checkpoint. Refreshing the show mapping flushes
both ESP32 caches while the other core is parked; a cache flush can disturb the
other core's in-flight cache fill even when it does not read the show mapping.
Partition-table storage is allocated on the heap.
Keep large flash scratch buffers out of the core-0 startup/network call stack:
saved-show restoration and upload persistence share that stack with filesystem
and SDK calls, independently of the core-1 renderer.

## Sequence editor playback

Open a sequence's export/device dialog, select the ordered controller outputs,
enter the device address and token, and choose **Connect to editor playback**.
Connections last for this desktop session and belong to the current project.
The dialog can disconnect and stop each device. Connecting stops its previous
standalone show before assigning the desktop clock master.

The normal editor Play button prepares each connected device's fragment, uploads
the whole archive when its SHA-256 or the device's loaded archive changes, and
schedules a shared future start. Unchanged project revisions reuse their compiled
fragment. This is local bytecode playback; no per-frame pixels are streamed.
Edits take effect on the next Play. Pause holds the position reached at its
scheduled deadline; seeking pauses at the new position and updates the home
position. Stop returns to home and outputs black. Rewind pauses at zero.
Playback ends at the sequence duration instead of looping; the standalone HTTP
Play endpoint still loops. Changing sequences or unloading stops the old show.

The desktop uses a monotonic clock. Eight authenticated UDP four-timestamp
exchanges subtract controller reply processing time and select the smallest
network round trip. The advertised `clockUdpPort` is separate from TCP on the
same port number. Requests contain `DCLK`, the 32 ASCII token bytes, and an
8-byte nonce. Replies contain `DCLK`, the echoed nonce, boot identity, and
receive/send timestamps (32 bytes total; integers are little endian).
Control JSON uses externally tagged commands: `{"syncClock": {...}}`,
`{"schedule": {...}}`, and `{"cancel": {"commandId": ...}}`. Their fields use
camel case. The embedded parser rejects missing or unknown command fields.
HTTP `GET /clock` remains a header-only diagnostic endpoint. Device TCP sockets
disable Nagle; show uploads and transport commands continue to use HTTP.
Scheduling requires an estimated one-way uncertainty of at most 2 ms, including
100 us allowance. Sync refreshes every five seconds; drift is estimated across
at least thirty seconds. Freshness expires after fifteen seconds for new
commands, while an already running show continues through a network outage.
Reboot identity, clock-master identity, command ordering, and archive CRC/size
are checked before accepting a scheduled command.

Commands use 250–1000 ms of lead time according to device count, round the deadline
up to the advertised output frame grid, and the desktop
requires acknowledgments with at least 40 ms remaining. Failed setup attempts
cancel all possibly armed commands and report any unconfirmed cancellation.
A lost network connection can prevent cancellation; this is not a distributed
atomic-start guarantee.

Controller frame selection follows elapsed show time, so missed evaluations skip
frames. Output transmission targets a shared 120 Hz clock grid and accounts for
the fixed WS281x data duration. Audio uses the backend's delayed-start/pause
commands and preview uses the same desktop deadline. The clock uncertainty is an
estimate that assumes approximately symmetric paths, not a measured physical
error bound. Single-digit-millisecond multi-controller light alignment and actual
speaker/output latency require hardware measurements; neither is established by
host tests or successful command acknowledgments.

## Build and verify

Export a representative selected fragment from the repository root:

```powershell
cargo run -p donder-elaboration --example export_sequence -- examples/starter firmware/esp32/target/loaded-sequence.donderseq
```

Configure the ESP tool paths from the [firmware prerequisites](../firmware/esp32/README.md#toolchain-and-dependencies).
Build from the repository root, then flash from `firmware/esp32`:

```powershell
pnpm firmware:cargo build --release --features i2s-output --bin loader --locked
cd firmware/esp32
espflash flash --port COM4 --baud 19200 --chip esp32 --non-interactive --flash-size 4mb --flash-mode dio --flash-freq 40mhz --partition-table partitions.csv --target-app-partition factory target/xtensa-esp32-none-elf/release/loader
```

Verify uploads, rejection behavior, and frame checksums:

```powershell
uvx --from esptool python upload.py target/loaded-sequence.donderseq --checksums target/loaded-sequence.donderseq.checksums --elf target/xtensa-esp32-none-elf/release/loader --windows-profile YOUR_PROFILE --uploads 3 --exercise-rejections --repeat 1 --monitor-seconds 75 --log target/i2s-playback.txt
```

Supplying `--checksums` is required for a frame-verification claim; an ordinary
upload only proves transport and admission. Supplying `--elf` records the exact
image hash. Captures belong under ignored output until reviewed. The promotion
policy and retained baseline are in [Performance and hardware evidence](performance.md).
Uploads remain stopped. Start playback through the desktop device controls or
authenticated `POST /transport/play` before claiming a continuous playback result.
The serial monitor option applies only to boards whose output pins leave UART0 free.

## Parallel WS281x output

### QuinLED Dig-Quad commissioning

For a Dig-Quad v2/v3 with the QuinLED classic ESP32 module, build the board-specific
image with `pnpm firmware:build --board dig-quad`. LED1 through LED4 use GPIO16,
GPIO3, GPIO1, and GPIO4. Confirm the module and board revision before installation;
other modules may swap GPIO1/GPIO3. This is not an automatically detected board.

Remove power and unplug the ESP32 module from the Dig-Quad before USB flashing
and provisioning. UART0 provisioning finishes and flushes before GPIO1/GPIO3
switch to LED output. The handoff disables the async UART interrupt and retains
UART0's clock for ROM transmitter calls. Application UART diagnostics are disabled;
serial playback capture is not supported on this board. ROM boot messages are not
controlled by the application. Disconnect USB before reinstalling the module.

The commissioning build limits output to 25/255 of authored channel values.
This is a brightness ceiling, not current measurement or estimated current
limiting. Device power configuration is still required before unrestricted output.
A configured board sends black frames even while Wi-Fi is reconnecting, and neither
upload nor reboot starts a saved show. Use the authenticated HTTP transport controls
to start playback explicitly. The four 150-pixel strands fit the current 200-pixel
per-lane output capacity.

### Reference-board output

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
