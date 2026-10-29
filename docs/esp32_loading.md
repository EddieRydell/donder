# ESP32 controllers

An ESP32 running the Donder loader plays shows from flash, either on
its own or in sync with the desktop editor. The loader runs `donder-runtime`, the
same engine as the preview, not a second interpreter. The host does all loading,
checking and preparation and sends a `.dondershow` show prepared for that
controller's output ports.

Supported hardware is a classic ESP32 with 4 MB flash and a 40 MHz crystal,
dual core and rated for 240 MHz, without flash encryption or secure boot.

## Set up a controller

1. Build and flash the firmware as described in the
   [firmware README](../firmware/esp32/README.md). The editor does not install
   firmware; it assumes a discovered controller already runs it.
2. Power the controller. With no saved network it hosts an open 2.4 GHz access
   point named `Donder-XXXX` (the last four hex digits of its MAC address) at
   `192.168.4.1`. Join it from the computer; its DHCP server offers no gateway,
   so a wired connection keeps the computer's internet route.
3. In Display Setup, add a **Donder controller**, choose the discovered device,
   and claim it. The first claim wins: the controller returns a token that the
   editor saves in its desktop settings, keyed by device ID. Every later request
   uses it. To claim it from another computer, factory reset it by reinstalling
   the firmware with its saved data erased (see the
   [firmware README](../firmware/esp32/README.md)).
4. Turn on live output in the sequence toolbar and press Play.

The controller's ID is its factory MAC address. A rename changes only the name
the editor shows, never the access point's name. **Join network** saves a
2.4 GHz WPA2 network and restarts; the controller then joins that network at
boot and falls back to its own access point if it cannot join within 20 seconds.

Reflash controllers whenever loader or show format code changes. The editor connects
only to controllers whose `format` record matches its own show format, and tells you to
update the firmware otherwise.

## Discovery

Each controller advertises `_donder._tcp` over mDNS as `donder-<id>.local`.
TXT records carry `id`, `name`, `claimed` (`0` or `1`), `format` and
`network` (`accessPoint` or `station`). The editor lists every advertised
controller. It browses only while live output is on or while a Donder
controller's settings or the controller form are open; otherwise it binds no
mDNS socket. While live output is on it connects to each Donder controller in
the open setup that it has claimed and that runs a matching format.

## Storage and upload

The configuration record (name, optional station network and claim token) lives
in LittleFS. Shows use two 128 KiB slots in a separate
256 KiB partition; a slot's first 4 KiB sector is its header, so a show may be
up to 124 KiB (`show_slots::CAPACITY`). A longer upload is refused with HTTP
422 and the show's size and that limit. An upload:
1. releases the current show, so its memory is free;
2. receives the show into RAM, if it fits beside the network reserve;
3. writes it to the inactive slot, erasing only the sectors it needs;
4. opens it and checks it against the controller;
5. commits the slot only after the show loads, and replies `LOADED`.

A failed or interrupted upload leaves no show loaded, but the previous committed
show stays saved. Reboot restores the newest committed slot, stopped. A saved
show that the current firmware rejects is left in place and the controller boots
without one; the editor uploads a current show on its next live Play.
An upload or configuration change waits for one in progress. The `donder`
partition is formatted at boot when it holds no usable filesystem, and a damaged
device record is replaced by defaults, so damaged storage never stops a
controller from starting.

The loader plays the show from RAM, beside its playback buffer, as the
`ShowBytes` an upload or a restore filled: reading it through the flash cache
cost Stanford `main` about a quarter of each frame. The playback stays
open while the show plays. The show is trusted: the editor built
it, so the loader checks only its format version. It rejects a show
- of another format version ("prepared for another firmware format");
- whose RAM copy, and then whose playback buffer, would leave less than
  10 KiB of heap for the network (`NETWORK_HEAP_RESERVE`; the buffer's size is exact, from the show's
  layout; Wi-Fi keeps four static receive buffers to leave heap for shows); the
  reason names the memory the show needs and the memory free;
- with more than 4 outputs, an output wider than 200 pixels (or not a whole
  number of pixels), or a higher frame rate than the outputs carry; the reason
  names those limits.

With no show loaded, about 77 KiB of heap is free for a show and its playback
buffer together. The
editor does not check a show against the controller before it uploads.

A panic prints `LOADER PANIC` and its message on the serial port (not on a
Dig-Quad, whose UART pins drive LEDs) and restarts the controller. Playback allocates nothing after the show loads.

A rejected show is answered with HTTP 422 and the reason. The
editor releases that controller and reports the reason in the live-output
status; the next Play reconnects and sends the show again.

The protocol's messages, paths and clock packets are defined once in
`crates/donder-device-protocol`, which the editor and the firmware both use.

HTTP endpoints, each but `POST /claim` requiring the claim token:
- unauthenticated `POST /claim`, which answers only an unclaimed controller;
- `PUT /show` and `POST /frame`;
- `PUT /name` with a 1-32 byte UTF-8 name, and `PUT /network` with
  `{"ssid": ..., "password": ...}` or an empty body for the access point;
- `GET /transport` and `POST /transport/play`, `/pause` or
  `/stop`;
- JSON `POST /control` for clock sync, scheduling and cancellation.

TCP sockets disable Nagle.

## Editor playback

A setup's Donder controller names its device ID and numbers its ports as outputs
1..n, which map in order onto the controller's physical outputs. Donder
controllers follow live output, like E1.31 and Art-Net controllers: with live
output off, Play only plays the Preview and the editor connects to no
controller. Turning live output on starts discovery; the editor then connects
to each claimed controller as it appears on the network and whenever the open
project changes, never during playback. Connecting stops the controller's
standalone show and synchronizes its clock. Turning live output off, opening
another sequence or project, or starting a performance stops and releases every
controller.

While live output is on, the editor's Play button:
1. connects any controller that is not yet connected, and prepares each
   connected controller's fragment;
2. uploads it if its id or the device's loaded show changed (unchanged
   project revisions reuse their compiled fragment);
3. schedules a shared future start.

A controller that is missing, unclaimed, on other firmware, or fails to connect,
take its show or acknowledge a schedule never blocks Play: the editor releases
it, names it with the reason in the live-output status and the controller's
settings, and plays the Preview and every other controller. The next Play
tries it again. With no controller connected, Play plays only the Preview.

Each controller evaluates its own frames, and no per-frame pixels cross the
network. Edits apply on the next Play. Pause holds the position reached at its
deadline, seek pauses at the new position, Stop returns home and blacks out, and
playback ends at the sequence duration. Each schedule carries the editor's
playback speed (show microseconds per wall second) and frame timing. Scaled
timing holds each show-time frame for longer when slowed down; constant
timing evaluates the exact show time at every output latch. Controllers send
frames at the sequence's frame rate; a show faster than the outputs can carry
(158 fps for 200 pixels a lane) is rejected at upload. With no show loaded the
controller sends black at 30 fps. Changing
speed during playback reschedules every controller and the audio from the
position reached at a new shared deadline. **Loop saved show** in the controller's
editor, or the standalone HTTP Play endpoint, loops the last uploaded show
without the editor instead.

Clock synchronization:
- **Exchanges.** Eight authenticated UDP four-timestamp exchanges, sent to the
  controller's own address on its HTTP port, keep the
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
  devices, plus eight times the slowest clock uncertainty per device, and round up to the sequence's frame grid. The desktop needs every
  acknowledgment with at least 40 ms to spare, or it cancels every command
  that may have been armed. A controller that fails to acknowledge has its
  command cancelled and is released.
- **Command checks.** Boot identity, clock-master identity, command order and
  the show's id and size are checked before a scheduled command is accepted,
  and its start must be at least 20 ms ahead. A clock estimate is accepted when
  its boot identity matches. The id is the hash the editor writes into the show
  header; the controller only reports and compares it.
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

- **Reference board** (`pnpm firmware:build`): GPIO13, GPIO18, GPIO21 and GPIO25.
- **QuinLED Dig-Quad v2/v3** (`pnpm firmware:build --board dig-quad`): LED1–LED4
  on GPIO16, GPIO3, GPIO1 and GPIO4. Confirm the module first, because other
  modules may swap GPIO1 and GPIO3.
  - Unplug the ESP32 module from the Dig-Quad to flash firmware over USB.
  - UART0 is handed over to LED output during boot, so serial capture is
    unavailable on this board.
  - This build caps output at 25/255 of authored values as a brightness ceiling,
    not a current limit.

Device timing and DMA completion do not show that LEDs lit. Hardware results state
separately whether LEDs or an oscilloscope were attached.
