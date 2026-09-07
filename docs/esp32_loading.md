# Prepared sequence loading on ESP32

The laptop elaborates only the selected controller outputs, serializes the resulting
`PreparedSequence`, and uploads that archive over HTTP. The ESP32 validates and
decodes the same type, creates its reusable workspace once, then evaluates it at a
requested time. Parsing, compilation, generator expansion, and elaboration remain
on the laptop. With the optional `i2s-output` feature, the same firmware also
evaluates continuously and drives four parallel WS281x outputs through I2S DMA.

## Transport

The firmware uses `picoserve` 0.20 with Embassy networking. There is no Dawn
chunking, acknowledgement, request parser, or raw TCP command protocol.

- `PUT /sequence` streams one binary archive into a bounded buffer. A successful
  decode atomically replaces the active sequence. Invalid or interrupted uploads
  leave the previous sequence active. Only one upload may allocate and decode at
  a time; a concurrent upload gets HTTP 409 while the other server worker remains
  available.
- `POST /frame` accepts one little-endian `u32` sample time and returns the frame
  CRC, evaluation time, and allocation counters. This endpoint is verification
  scaffolding; scheduled playback will call the runtime directly.
- Both endpoints require the per-boot `X-Dawn-Token` header.

`picoserve` supplies HTTP parsing, method/path dispatch, body framing and IO
timeouts. Its default three-second whole-request timeout was too short for the
32 KiB device limit on variable Wi-Fi, so only an authenticated sequence body gets
a 15-second timeout. Two library-managed workers allow keep-alive without letting
one client own the only socket, and let a fresh connection proceed while an
interrupted body expires. Persistent-request and response-write deadlines are
five seconds.

USB serial is used only to provision Wi-Fi and return the DHCP address and random
128-bit HTTP token. The host initiates this exchange with `P`; it no longer depends
on a one-shot boot greeting. This fixes the observed false startup timeout, where
the board was alive but the first characters of its unsolicited greeting were
damaged. Boot logging may still be corrupted and is not part of the protocol.

Credentials, token, and the active sequence are currently RAM-only, so reset still
requires provisioning and upload. Persistent credential/sequence storage and a
real provisioning UI remain product work. HTTP and the token are plaintext and are
appropriate only on a trusted LAN; this is not an Internet-facing service.

## Archive and memory

`crates/dawn-runtime/src/wire.rs` owns `encode_sequence`, `decode_sequence`,
`LoadLimits`, and `LoadError`. Runtime and codec remain `no_std + alloc`. The
16-byte header contains `DAWN`, the current `u32` version 5, payload length, and CRC32. The
payload is a 32-bit, little-endian rkyv archive validated before deserialization.
CRC detects corruption, not authenticity.

The current representation has no redundant prepared gradient/forward-curve tables. Native effects,
DSL values and controls share forward sampling rules; prepared curve crossings
remain available for inverse queries. Regenerate archives when updating firmware.
Workspace admission now uses the actual register/value sizes and reserved layouts,
including temporal frame storage. It remains a conservative estimate, not an OOM
proof or a sandbox for arbitrary bytecode.

Firmware limits are currently 32 KiB of payload, 1,600 pixels, 128 graph nodes,
and a 96 KiB estimated workspace. Loading allocates; successful frame evaluation
does not. The loader-only build reserves eight times the archive size for decoding
headroom. The I2S build instead derives the workspace limit from the heap remaining
after the upload buffer and any active sequence, leaving 16 KiB untouched. This
allows an archive to replace the active sequence without stopping output. These
are admission policies, not proofs against OOM.

The current four-port starter fragment is 25,361 bytes for 452 pixels and ten
selected effects. It contains effects and prepared sequence data, not precomputed
RGB frames.

The classic ESP32 has 520 KiB of on-chip SRAM, but it is split among executable
data, static data, stacks, caches and heap; it is not a 520 KiB application heap.
The I2S build explicitly provides a 160 KiB allocator. Its two 15,120-byte DMA
buffers are static and hold one complete 200-pixel transmission each, including a
300-us low reset. The September 7 run reported 117,268 heap bytes free before
loading and a minimum of 81,208 during the representative playback capture.
Frame evaluation allocated nothing; network tasks can allocate independently.
See [the current measurements](signal_model_work.md) for exact images, payloads,
the 10,800-frame deadline result, and live-generator workload validation.

A representative synthetic 100-effect fragment, made from the starter's Spin
and Pulse effects over the same 113 pixels with one shared program, encoded to
26,014 bytes. Its decoded sequence, workspace and output retained 43,348 bytes,
and decoding peaked at 57,232 bytes in addition to the resident archive. This is
not a universal per-effect multiplier: parameters, automations, targets and unique
programs all affect the size. It also means that the current RAM-buffered loader
may still exceed the current firmware's replacement-time headroom. Flash-backed
upload staging or decoding without retaining the whole archive remains the next
memory improvement for substantially larger fragments.

## Build and verify

Export the selected first four controller outputs from the repository root:

```powershell
cargo run -p dawn-elaboration --example export_sequence -- examples/starter firmware/esp32/target/loaded-sequence.dawnseq
```

Then run from `firmware/esp32`:

```powershell
. ./export-esp.ps1
cargo +esp build --release --features loader --bin loader --locked
espflash flash --port COM4 --baud 19200 --chip esp32 --non-interactive --flash-size 4mb --flash-mode dio --flash-freq 40mhz target/xtensa-esp32-none-elf/release/loader
uvx --from esptool python upload.py target/loaded-sequence.dawnseq --windows-profile YOUR_PROFILE --uploads 3 --exercise-rejections --log results/http-load.txt
```

Use `--features i2s-output` instead of `loader` to run continuous 120 Hz playback
and four concurrent 200-pixel outputs on GPIO13, GPIO18, GPIO21 and GPIO25. This
starts a second Embassy executor: Wi-Fi/HTTP stays on core 0, while sequence
evaluation, parallel encoding and I2S DMA run on core 1. Dawn's
`atomic` feature is enabled only for this multicore build so the immutable shared
sequence and its workspace can legally cross the core boundary; frame evaluation
does not clone or drop those shared references.

I2S runs at 2.4 MHz. Each WS281x bit is encoded as exactly three samples: `100`
for zero or `110` for one, giving a constant 1.25-us bit cell. One byte carries the
state of up to eight output lanes at a sample instant. The current firmware uses
four lanes and double buffering: while DMA transmits one complete frame, the CPU
evaluates and encodes the next into the other buffer. There are no mid-frame CPU
refills.

Alternatively use `--ssid YOUR_2_4_GHZ_SSID`; the tool prompts for its password
without echo. On English Windows, `--windows-profile` reads the selected saved
personal-network profile in memory. Passwords and tokens are never logged or
written to evidence files.

## Current validation and historical measurements

See [the signal model record](signal_model_work.md) for the current runtime,
archive, loader image, live generators, and representative I2S measurements.
The earlier [execution audit](execution_audit_2026-09-06.md) records six 200-pixel
fixtures with stacked chases and generated marks through the same uploader. The
`/frame` handler releases its playback lock before sending the HTTP response;
frame verification still shares the workspace with continuous playback.

The measurements below are historical, recording individual earlier changes;
their image hashes and timings do not describe the latest runtime.

The accepted loader-only hardware run used loader ELF SHA256
`47020836d08efa737712aaf707e163532268563524c2b69191f6428d66bf14f1`
and payload SHA256
`89bb791bc2978904c4335f391c72a3a293be5d823de54f737aa4dbe3c21a8bef`.
Three HTTP uploads completed in 0.312, 0.094, and 0.094 seconds; decode, workspace,
and output construction took 7.201-11.441 ms. The device rejected an invalid token,
bad version, oversized declared payload, bad checksum, and interrupted body. It
also rejected a concurrent upload. It then evaluated 200 host-selected frames
correctly with zero evaluation allocations, proving that failed uploads retained
the active sequence. See the
[capture](../firmware/esp32/results/2026-09-05-picoserve-http-final3.txt).

The `/frame` timing includes radio interrupts, HTTP handling immediately before
evaluation, and cold instruction-cache effects. Continuous Wi-Fi-free profiling
showed the SDK migration within -1% to +1.5% across all 15 fixtures, including
stacked chases and mark effects; use that result rather than HTTP request timing
as the indication of that migration's steady playback performance. Historical measurements
and their limitations remain in `runtime_optimization_2026-09-05.md`.

The firmware currently pins the coordinated esp-rs SDK revision documented in
`firmware/esp32/README.md`.

That dual-core Wi-Fi/I2S image had ELF SHA256
`32a4312b56bb6644f714ced974a2e69bd7545aa06a7468518467874ad9827e2a`.
Three consecutive 25,025-byte replacements succeeded in 0.156-0.453 seconds.
Two hundred authenticated HTTP frame requests returned the expected checksums,
all rejection cases passed, and evaluation performed zero allocations.

Ordinary playback windows evaluate the four-port fragment in about 1.87-1.90 ms,
encode it in about 2.47 ms, and finish the overlapped DMA frame in about 6.35 ms,
with zero missed 8.333-ms deadlines. At 58.98-60.54 seconds, two per-fixture Spin
effects with six revolutions overlap; at 64.32-67.05 seconds, a 25-revolution
per-fixture Spin is active. These regions previously reached 22-37 ms because a
pixel-uniform TimeWarp read sampled identical per-fixture pixels separately for
all four fixtures. The compiler now marks uniform signal times, and the runtime
fills and reuses one upstream frame per marked read. Together with removing
repeated Spin divisions, direct 66-second evaluation is now 6.210 ms on average
(5.624-8.830 ms over 20 HTTP samples), down from 39.840 ms on average
(38.126-44.827 ms). Ordinary playback is unchanged. The worst continuous window
now averages 6.476 ms of evaluation and still misses some total deadlines once
the 2.47-ms encoding step is included; the later high-revolution windows average
about 3.8-4.8 ms and stay under the deadline. Frame evaluation still allocates
nothing. These remaining misses are runtime effect cost, not I2S refill jitter.
No LEDs or oscilloscope were connected, so this verifies real I2S DMA completion
on the GPIO peripheral but not external waveform voltage or LED behavior. See the
[current combined capture](../firmware/esp32/results/2026-09-05-temporal-frame-cache-final.txt),
[previous playback capture](../firmware/esp32/results/2026-09-05-i2s-full-playback.txt),
and [previous HTTP verification](../firmware/esp32/results/2026-09-05-i2s-http-200.txt).

The subsequent shared-runtime geometry hoist computes Chase/Spin section count,
revolution scale, pulse duration, reciprocal duration and extension bounds once
per active effect and fixture geometry. It does not add a firmware-specific path,
heap storage, or evaluation allocations. The direct 66-second frame improved again
to 5.674 ms average and 5.343 ms median over 20 HTTP samples (5.258-8.544 ms;
5.452-ms mean with the two radio-interrupted outliers removed). The worst continuous
windows improved from 6.476/5.526 ms to 6.014/5.142 ms evaluation, and the later
25-revolution window improved from 4.819 ms to 4.470 ms. Ordinary evaluation is
about 1.84-1.86 ms. The first hot region remains just over the full deadline after
the unchanged 2.46-ms encoder: 8.538 ms average total. The measured image SHA256 is
`d791c8042d2ed0ceae4eecbd94782e0cb29c2c3e791c6bd00601007b13b7488d`; see the
[geometry-hoist capture](../firmware/esp32/results/2026-09-05-hoisted-native-geometry.txt).
