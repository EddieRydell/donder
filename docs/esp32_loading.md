# ESP32 controllers

An ESP32 running the Donder loader plays prepared sequences from flash, either on
its own or in sync with the desktop editor. The loader runs `donder-runtime`, the
same engine as the preview, not a second interpreter. The host does all loading,
checking and preparation and sends a `.donderseq` archive prepared for that
controller's output ports.

Supported hardware is a classic ESP32 with 4 MB flash and a 40 MHz crystal,
dual core and rated for 240 MHz, without flash encryption or secure boot. The
desktop installer checks all of this before writing.

## Install from Donder

1. Connect the board over USB and install the bundled firmware from the
   controller editor. The image is
   `apps/desktop/assets/firmware/donder-esp32.bin`; the installer checks its
   SHA-256 and that its partition table matches `firmware/esp32/partitions.csv`.
   Installation does not overwrite the credential and show partitions.
2. Provision the board's 2.4 GHz Wi-Fi credentials from Donder. Provisioning
   returns a device token over USB serial, stored with the credentials; uploads
   and control requests use it.
3. Export a compiled sequence for the controller's ports and upload it. Uploaded
   and restored shows stay stopped and black until **Play** is requested.

Rebuild the bundled image with `pnpm firmware:build` and commit it whenever
loader or archive code changes. The desktop rejects a controller whose
`sequence_format` differs from its own, but only after installation.

## Storage and upload

Credentials live in LittleFS. Archives use two 128 KiB slots in a separate
256 KiB partition. An upload:
1. blacks out output and releases the current show;
2. streams the archive to the inactive slot;
3. validates and decodes it directly from memory-mapped flash;
4. commits the slot only after admission succeeds.

A failed or interrupted upload leaves no show loaded, but the previous committed
archive stays saved. Reboot restores the newest committed slot, stopped.
Concurrent uploads are rejected, and an explicit erase clears both partitions.

Admission checks the header, version and CRC, the archive structure, and these
limits:
- 96 KiB of payload;
- 1,600 pixels;
- 128 graph nodes;
- 96 KiB of estimated workspace, derived from the remaining heap.

It also checks bytecode register references, operand spans, constants, jump
targets, parameter types and return paths, signal reads within an operator's
inputs, and prepared automation mappings. The limits are conservative policy,
not a proof that memory will never run out.

HTTP endpoints:
- `GET /capabilities`, `PUT /sequence` and `POST /frame`;
- with output enabled, `GET /transport` and `POST /transport/play`, `/pause` or
  `/stop`;
- authenticated `GET /clock` and JSON `POST /control` for clock sync, scheduling
  and cancellation.

TCP sockets disable Nagle.

## Editor playback

In a sequence's export dialog, select each controller's ordered outputs, enter
its address and token, and choose **Connect to editor playback**. Connections
last for the desktop session and belong to the current project. Connecting stops
the controller's standalone show.

The editor's Play button then:
1. prepares each connected controller's fragment;
2. uploads it if its SHA-256 or the device's loaded archive changed (unchanged
   project revisions reuse their compiled fragment);
3. schedules a shared future start.

Each controller evaluates its own frames, and no per-frame pixels cross the
network. Edits apply on the next Play. Pause holds the position reached at its
deadline, seek pauses at the new position, Stop returns home and blacks out, and
playback ends at the sequence duration. The standalone HTTP Play endpoint loops
instead.

Clock synchronization:
- **Exchanges.** Eight authenticated UDP four-timestamp exchanges keep the
  smallest round trip, after subtracting the controller's reply processing
  time. Requests carry `DCLK`, the 32-byte token and an 8-byte nonce. Replies
  carry `DCLK`, the nonce, a boot identity and receive/send timestamps (32
  bytes, little endian).
- **Control commands.** JSON commands are externally tagged (`syncClock`,
  `schedule`, `cancel`) with camel-case fields; unknown fields are rejected.
- **Freshness.** Sync refreshes every 5 s, and drift is estimated over at least
  30 s. Scheduling needs an estimated one-way uncertainty of at most 2 ms. A
  sync older than 15 s blocks new commands, but a running show continues
  through a network outage.
- **Scheduling.** Starts use 250–1000 ms of lead time depending on the number of
  devices and round up to the 120 Hz output frame grid. The desktop needs every
  acknowledgment with at least 40 ms to spare. A failed setup cancels every
  command that may have been armed.
- **Command checks.** Boot identity, clock-master identity, command order and
  archive CRC and size are checked before a scheduled command is accepted.
- **Playback timing.** Frames follow elapsed show time, so a late evaluation
  skips frames rather than drifting.

The uncertainty estimate assumes roughly symmetric network paths. Physical
multi-controller alignment and speaker latency have not been measured.

## Output

Sequence evaluation, encoding and DMA run on core 1; Wi-Fi and HTTP on core 0.
Four WS281x outputs of up to 200 RGB pixels each are driven by I2S1 in 8-bit
parallel mode at 2.4 MHz. Each bit is three samples (`100` for zero, `110` for
one), and two DMA buffers let the next frame be encoded while the current one
transmits.

- **Reference board** (`i2s-output`): GPIO13, GPIO18, GPIO21 and GPIO25.
- **QuinLED Dig-Quad v2/v3** (`pnpm firmware:build --board dig-quad`): LED1–LED4
  on GPIO16, GPIO3, GPIO1 and GPIO4. Confirm the module first, because other
  modules may swap GPIO1 and GPIO3.
  - Unplug the ESP32 module from the Dig-Quad to flash and provision it over USB.
  - UART0 is handed over to LED output after provisioning, so serial capture is
    unavailable on this board.
  - This build caps output at 25/255 of authored values as a brightness ceiling,
    not a current limit.

  The Dig-Quad build also overwrites the bundled desktop image.

Device timing and DMA completion do not show that LEDs lit. Hardware results state
separately whether LEDs or an oscilloscope were attached.
