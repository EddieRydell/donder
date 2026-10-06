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

1. Connect the board over USB and install the bundled firmware from **USB setup**
   in a Donder controller's editor or the new-controller form. The image is
   `apps/desktop/assets/firmware/donder-esp32.bin`; the installer checks its
   SHA-256 and that its partition table matches `firmware/esp32/partitions.csv`,
   then writes at 460,800 baud in about half a minute. Installation keeps the
   configuration and show partitions.
2. Power the controller. With no saved network it hosts an open 2.4 GHz access
   point named `Donder-XXXX` (the last four hex digits of its MAC address) at
   `192.168.4.1`. Join it from the computer; its DHCP server offers no gateway,
   so a wired connection keeps the computer's internet route.
3. In Display Setup, add a **Donder controller**, choose the discovered device,
   and claim it. The first claim wins: the controller returns a token that the
   editor saves in its desktop settings, keyed by device ID. Every later request
   uses it. A USB factory reset erases the claim.

The controller's ID is its factory MAC address. A rename changes only the name
the editor shows, never the access point's name. **Join network** saves a
2.4 GHz WPA2 network and restarts; the controller then joins that network at
boot and falls back to its own access point if it cannot join within 20 seconds.

Rebuild the bundled image with `pnpm firmware:build` and commit it whenever
loader or archive code changes. The editor connects only to controllers that
advertise its own `sequence_format`.

## Discovery

Each controller advertises `_donder._tcp` over mDNS as `donder-<id>.local`.
TXT records carry `id`, `name`, `claimed` (`0` or `1`), `format` and
`network` (`accessPoint` or `station`). The editor lists every advertised
controller; it connects editor playback to each Donder controller in the open
setup that it has claimed and that runs a matching format.

## Storage and upload

The configuration record (name, optional station network and claim token) lives
in LittleFS. Archives use two 128 KiB slots in a separate
256 KiB partition. An upload:
1. blacks out output and releases the current show;
2. streams the archive to the inactive slot;
3. validates and decodes it directly from memory-mapped flash;
4. commits the slot only after admission succeeds.

A failed or interrupted upload leaves no show loaded, but the previous committed
archive stays saved. Reboot restores the newest committed slot, stopped. A saved
show that the current firmware rejects is left in place and the controller boots
without one; the editor uploads a current show on its next Play.
Concurrent uploads are rejected, and an explicit erase clears both partitions.

Admission checks the header, version and CRC, the archive structure, and these
limits:
- 96 KiB of payload;
- 1,600 pixels;
- 128 graph nodes;
- 96 KiB of estimated workspace, and no more than the heap left after decoding
  minus 10 KiB for the network. The loader decodes once to measure that, then
  again to admit. Wi-Fi keeps four static receive buffers to leave heap for
  shows.

It also checks bytecode register references, operand spans, constants, jump
targets, parameter types and return paths, signal reads within an operator's
inputs, and prepared automation mappings. The limits are conservative policy,
not a proof that memory will never run out.

HTTP endpoints:
- unauthenticated `POST /claim`, which answers only an unclaimed controller;
- `GET /capabilities`, `PUT /sequence` and `POST /frame`;
- `PUT /name` with a 1-32 byte UTF-8 name, and `PUT /network` with
  `{"ssid": ..., "password": ...}` or an empty body for the access point;
- with output enabled, `GET /transport` and `POST /transport/play`, `/pause` or
  `/stop`;
- authenticated `GET /clock` and JSON `POST /control` for clock sync, scheduling
  and cancellation.

TCP sockets disable Nagle.

## Editor playback

A setup's Donder controller names its device ID and numbers its ports as outputs
1..n, which map in order onto the controller's physical outputs. The editor
connects to each claimed controller as it appears on the network and whenever
the open project changes, never during playback. Connecting stops the
controller's standalone show and synchronizes its clock.

The editor's Play button then:
1. prepares each connected controller's fragment;
2. uploads it if its SHA-256 or the device's loaded archive changed (unchanged
   project revisions reuse their compiled fragment);
3. schedules a shared future start.

Each controller evaluates its own frames, and no per-frame pixels cross the
network. Edits apply on the next Play. Pause holds the position reached at its
deadline, seek pauses at the new position, Stop returns home and blacks out, and
playback ends at the sequence duration. **Loop saved show** in the controller's
editor, or the standalone HTTP Play endpoint, loops the last uploaded show
without the editor instead.

Clock synchronization:
- **Exchanges.** Eight authenticated UDP four-timestamp exchanges keep the
  smallest round trip, after subtracting the controller's reply processing
  time. Requests carry `DCLK`, the 32-byte token and an 8-byte nonce. Replies
  carry `DCLK`, the nonce, a boot identity and receive/send timestamps (32
  bytes, little endian).
- **Control commands.** JSON commands are externally tagged (`syncClock`,
  `schedule`, `cancel`) with camel-case fields; unknown fields are rejected.
- **Freshness.** Sync refreshes every 5 s, and drift is estimated over at least
  30 s. The editor shows the estimated one-way uncertainty but never refuses to
  play over a slow network; a larger value only loosens light-to-audio sync. A
  sync older than 15 s blocks new commands, but a running show continues
  through a network outage.
- **Scheduling.** Starts use 250–1000 ms of lead time depending on the number of
  devices, plus eight times the slowest clock uncertainty per device, and round up to the 120 Hz output frame grid. The desktop needs every
  acknowledgment with at least 40 ms to spare. A failed setup cancels every
  command that may have been armed.
- **Command checks.** Boot identity, clock-master identity, command order and
  archive CRC and size are checked before a scheduled command is accepted.
- **Playback timing.** Frames follow elapsed show time, so a late evaluation
  skips frames rather than drifting.

The uncertainty estimate assumes roughly symmetric network paths. Physical
multi-controller alignment and speaker latency have not been measured.

## Output

Sequence evaluation, encoding and DMA run on core 1; Wi-Fi, HTTP, DHCP, mDNS and
flash storage on core 0. Core 1 starts before the network, so joining a missing
network never delays a restored show. The UDP buffers for DHCP and mDNS live in
the 8 KiB RTC fast memory, which only core 0 can address, leaving main DRAM to
the core 0 stack and the show heap.
Four WS281x outputs of up to 200 RGB pixels each are driven by I2S1 in 8-bit
parallel mode at 2.4 MHz. Each bit is three samples (`100` for zero, `110` for
one), and two DMA buffers let the next frame be encoded while the current one
transmits.

- **Reference board** (`i2s-output`): GPIO13, GPIO18, GPIO21 and GPIO25.
- **QuinLED Dig-Quad v2/v3** (`pnpm firmware:build --board dig-quad`): LED1–LED4
  on GPIO16, GPIO3, GPIO1 and GPIO4. Confirm the module first, because other
  modules may swap GPIO1 and GPIO3.
  - Unplug the ESP32 module from the Dig-Quad to install firmware or factory
    reset it over USB.
  - UART0 is handed over to LED output one second after boot, so serial capture
    is unavailable on this board.
  - This build caps output at 25/255 of authored values as a brightness ceiling,
    not a current limit.

  The Dig-Quad build also overwrites the bundled desktop image.

Device timing and DMA completion do not show that LEDs lit. Hardware results state
separately whether LEDs or an oscilloscope were attached.
