# ESP32 controllers

An ESP32 running the Donder loader plays what the desktop editor plays, in
sync with it. The loader runs `donder-runtime`, the
same engine as the preview, not a second interpreter. The host does all loading,
checking and preparation and sends a `.dondershow` show prepared for that
controller's output ports.

Supported hardware is a classic ESP32 with 4 MB flash and a 40 MHz crystal,
dual core and rated for 240 MHz, without flash encryption or secure boot.

## Set up a controller

1. Build and flash the firmware as described in the
   [firmware README](../firmware/esp32/README.md). The editor does not install
   firmware; it assumes a discovered controller already runs it.
2. Power the controller. With no saved network it hosts a 2.4 GHz WPA2 access
   point named `Donder-XXXX` (the last four hex digits of its MAC address),
   password `12345678` (`ACCESS_POINT_PASSWORD`), at `192.168.4.1`. Join it
   from the computer; its DHCP server offers no gateway, so a wired connection
   keeps the computer's internet route.
3. In the project overview, add a **Donder controller** and choose the discovered
   device.
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
TXT records carry `id`, `name`, `format` and
`network` (`accessPoint` or `station`). The editor lists every advertised
controller. It browses only while live output is on or while a Donder
controller's settings or the controller form are open; otherwise it binds no
mDNS socket. While live output is on it connects to each Donder controller in
the project that runs a matching format.

## Storage and shows

The configuration record (name and optional station network) lives
in LittleFS in the `donder` partition. The partition is formatted at boot when
it holds no usable filesystem, and a damaged device record is replaced by
defaults, so damaged storage never stops a controller from starting.

Shows live only in RAM: a controller boots with none, and the editor sends the
show it plays whenever the controller lacks it. A show and its playback buffer
share the show region, the 64 KiB of RAM the bootloader used
(`donder_device_protocol::SHOW_REGION_BYTES`): the show's bytes first, then the buffer, whose size is
exact, from the show's layout. Nothing else allocates there, so the heap's
fragmentation never limits a show; the 62 KiB heap is the network's, plus a
console's routing state. Receiving a show (`PUT /show`):
1. stops the current show and takes back its region;
2. receives the show into the region;
3. opens it and checks it against the controller, and replies `LOADED`.

A failed or interrupted show leaves none loaded. One show is received at a time.

A delta (`PUT /show/delta`, `donder_device_protocol::delta`) turns the show the
controller holds into another in place, between two frames, and the show keeps
its transport: it plays on from where it was. The delta names the show it
applies to, which must be the one the controller holds (or the reply is 409
Conflict), and the show it makes, whose identity is checked after it applies.
Its operations keep, drop and insert runs of the show's 32-bit words, front to
back; kept runs move in two passes, those moving toward the start first and
then those moving toward the end, so no run is overwritten before it moves. A
delta that fails after applying leaves no show loaded, and the editor sends
the whole show instead.

The show is trusted: the editor built it, so the loader checks only its
format version and length. It rejects a show
- of another format version ("prepared for another firmware format");
- that, with its playback buffer, does not fit in the show region; the reason
  names the memory the show needs and the region's size;
- with more than 4 outputs, an output wider than 200 pixels (or not a whole
  number of pixels), or a higher frame rate than the outputs carry; the reason
  names those limits.

Before sending a show, the editor checks that it and its playback buffer fit
the show region (`PreparedShow::size` breaks both down by part). A show too
large is not sent; the controller's reason names the total, the region's size
and the three largest parts, each with what shrinks it: for example the
frames playback holds for the layers, operators and scans lit at once, the
clips, or the pixels' positions. The controller's own check stays.

A panic prints `LOADER PANIC` and its message on the serial port (not on a
Dig-Quad, whose UART pins drive LEDs) and restarts the controller. Playback allocates nothing after the show loads.

A rejected show is answered with HTTP 422 and the reason. The
editor releases that controller and reports the reason in the live-output
status; the next transport change reconnects and sends the show again.

The protocol's messages, paths and clock packets are defined once in
`crates/donder-device-protocol`, which the editor and the firmware both use.

HTTP endpoints, none authenticated: anyone on the controller's network can
control it, so the access point's password is the only access control.
- `PUT /show`, `PUT /show/delta` and `POST /frame`;
- `PUT /name` with a 1-32 byte UTF-8 name, and `PUT /network` with
  `{"ssid": ..., "password": ...}` or an empty body for the access point;
- `GET /transport` for the loaded show's playback, and `POST
  /transport/stop`, which stops it when the editor releases the controller;
- JSON `POST /control` for clock sync and scheduling.

TCP sockets disable Nagle.

## Editor playback

A project's Donder controller names its device ID and numbers its ports as outputs
1..n, which map in order onto the controller's physical outputs. Donder
controllers follow live output, like E1.31 and Art-Net controllers: with live
output off, Play only plays the Preview and the editor connects to no
controller. Turning live output on starts discovery; the editor then connects
to each controller as it appears on the network, whenever the open project
changes and with every transport change. Connecting stops whatever the
controller plays and synchronizes its clock. Turning live output off, opening
another sequence or project, or stopping a performance stops and releases every
controller; opening another sequence while performing does not.

Transport commands (play, pause, stop, seek, speed, range and loop) act on
the editor's audio and Preview at once. While live output is on, each one
then has the controllers follow the editor's transport on a background task
that always takes the latest request:
1. it connects any controller that is not yet connected, and prepares each
   connected controller's show;
2. it rebuilds the show for a later project revision and sends it unless the
   controller holds it: as a delta of the show the controller holds when that
   is less than half the show, so an edit applies while the show plays, or
   whole otherwise, when the controller goes dark until it loads;
3. it schedules every controller to take up the transport's place a lead time
   from now (see Scheduling below), so each joins in sync a few frames after
   the editor;
4. while playing, it schedules the wrap or stop at the next loop or range end
   on the controllers' clock, and the playback poll schedules the one after
   once it passes.

A controller that is missing, on other firmware, or fails to connect, take
its show or accept a schedule never holds up the editor: it is released and
named with the reason in the live-output status and the controller's
settings, and the next transport change tries it again.

Each controller evaluates its own frames, and no per-frame pixels cross the
network. Edits apply as you make them. Pause holds the position the
editor paused at, seek pauses at the new position, Stop returns home and blacks
out, and playback ends at the sequence duration. Each schedule carries the editor's
playback speed (show microseconds per wall second) and frame timing. Scaled
timing holds each show-time frame for longer when slowed down; constant
timing evaluates the exact show time at every output latch. Controllers send
frames at the sequence's frame rate; a show faster than the outputs can carry
(158 fps for 200 pixels a lane) is rejected at upload. With no show loaded the
controller sends black at 30 fps.

## Performing

While a performance plays with live output on, the editor connects each Donder
controller as it does for a sequence, then plays the console on it: it
prepares the controller's part of the console, uploads it unless the
controller holds it, and schedules every connected controller to play from the
session clock's position at a shared deadline. A controller that has not yet
joined this console then opens a TCP connection to port 81
(`donder_device_protocol::console`) and receives the performance's inputs:
13-byte messages, each a kind and an encoded `ConsoleEvent` (time in ticks,
control, happening, note or 255, value). Each connection plays the loaded
console from its start; `Replayed` inputs apply at their times, and `Live`
inputs at the show time they arrive, never earlier than the input before.
Routing runs on the controller, like on the editor. An edit arms the console
again and every controller joins again, taking an upload when its part
changed. When a connection ends, or stops answering keepalives for 6 s, the
controller releases every held control; the editor reconnects each second and
replays every input so far.

Clock synchronization:
- **Exchanges.** Eight UDP four-timestamp exchanges, sent to the
  controller's own address on its HTTP port, keep the
  smallest round trip, after subtracting the controller's reply processing
  time. Requests carry `DCLK` and an 8-byte nonce. Replies
  carry `DCLK`, the nonce, a boot identity and receive/send timestamps (32
  bytes, little endian).
- **Control commands.** JSON commands are externally tagged (`syncClock`,
  `schedule`) with camel-case fields; unknown fields are rejected.
- **Freshness.** Sync refreshes every 5 s, and drift is estimated over at least
  30 s. The editor shows the estimated one-way uncertainty but never refuses to
  play over a slow network; a larger value only loosens light-to-audio sync. A
  sync older than 15 s blocks new commands, but a running show continues
  through a network outage.
- **Scheduling.** A schedule takes effect the controller's 20 ms lead
  (`SCHEDULE_LEAD_MICROS`) after it is sent, plus the slowest controller's
  recent command round trip and clock uncertainty. The editor sends it to every
  controller at once, with the position the transport reaches at that moment.
  Each controller holds one pending command, which a later one replaces. A
  controller that does not accept a schedule is released.
- **Command checks.** Boot identity, clock-master identity, command order and
  the show's id and size are checked before a scheduled command is accepted,
  and its start must be at least `SCHEDULE_LEAD_MICROS` ahead. A clock estimate is accepted when
  its boot identity matches. The id is the hash the editor writes into the show
  header; the controller only reports and compares it.
- **Playback timing.** Frames follow elapsed show time, so a late evaluation
  skips frames rather than drifting.

The uncertainty estimate assumes roughly symmetric network paths. Physical
multi-controller alignment and speaker latency have not been measured.

## Output

Sequence evaluation, encoding and DMA run on core 1; Wi-Fi, HTTP, DHCP, mDNS and
flash storage on core 0. Core 1 starts before the network, so joining a missing
network never delays output. The UDP buffers for DHCP and mDNS live in
the 8 KiB RTC fast memory, which only core 0 can address, leaving main DRAM to
the core 0 stack and the network heap.
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

Device timing and DMA completion do not show that LEDs lit. Hardware results state
separately whether LEDs or an oscilloscope were attached.
