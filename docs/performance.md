# Performance and hardware evidence

Donder treats representative playback cost, frame deadlines, and retained memory
as acceptance evidence. Isolated microbenchmarks help locate work but do not
justify complexity by themselves. The repeatable Criterion workflow is in
[Regression tracking](regression_tracking.md).

## Retained baseline

The reviewed ESP32 captures under
`firmware/esp32/results/accepted` are a September 6, 2026 baseline for the image
and payload hashes recorded in each file. They are not proof about newer source
or a newly built firmware image.

The accepted I2S capture verifies 200 host-selected frame checksums and records
109 playback windows, representing 13,080 frames at 120 Hz with no missed
deadlines. Ordinary windows took about 1.83-1.84 ms to evaluate, about 2.48 ms to
encode, and about 6.35 ms for the overlapped DMA frame; the largest recorded
complete frame was 7.939 ms. Six controller-shaped fixture captures verify 576
additional frame checksums with zero evaluation allocations.

These runs exercised the ESP32 I2S peripheral and DMA completion. No LEDs or
oscilloscope were connected, so they do not verify external voltage levels,
waveform shape, signal integrity, or visible output. Network tasks can allocate
independently even though frame evaluation recorded zero allocations.

## Scheduled controller playback

The [September 30 Dig-Quad result](../firmware/esp32/results/accepted/2026-09-30-dig-quad-scheduled-playback.json)
identifies the tested firmware and 600-pixel archive by SHA-256. The final
board-specific image passed upload replacement, scheduled start, cancellation,
elapsed position, pause, seek, and stop. The desktop's real-controller transport
test also passed Play, Pause, Seek, Stop, and unchanged replay.

The best UDP round trip was 2.316 ms, giving an estimated clock uncertainty of
1.258 ms under the approximately symmetric-path assumption. These checks used
the removed ESP32 module on USB power. They establish controller transport
behavior, not visible LED output, multi-controller physical skew, or speaker
latency; the test sequence had no audio file.

The [October 1 Stanford result](../firmware/esp32/results/accepted/2026-10-01-dig-quad-stanford-playback.json)
records the full 78,392-byte show mapped to 150 serial pixels on Dig-Quad LED1.
Repeated full-show replacement, damaged-upload rejection, ten desktop/device
frame checksums, scheduled transport, and desktop transport with audio passed.
Replacement releases the old decoded show before receiving the new body and
validates the candidate once. The decoded sequence uses 93,860 bytes; its workspace
uses 4,228 bytes, and retained playback including outputs uses 98,552 bytes.
Frame evaluation recorded zero allocations. Active sampled frames took
41.5–48.9 ms, so this workload cannot produce 120 distinct frames per second;
elapsed-time playback skips missed frames. The best UDP round trip was 2.610 ms,
with estimated clock uncertainty of 1.405 ms. After reboot the show restored
stopped, and a scheduled preview traversed its colored 70–82-second section.
The checksum and desktop checks used the removed module on USB power. After
reinstallation, the user confirmed visible Stanford playback on LED1 during a
second scheduled color preview. Physical clock skew and audio/LED alignment
remain unmeasured.

## Retention policy

Raw captures, failed uploads, superseded profiles, generated archives, checksum
sidecars, and Criterion output are build artifacts and stay in ignored output
directories. A capture is promoted to `results/accepted` only when its collector
completed, hashes/checksums matched, and the smallest useful evidence file was
reviewed. Never repair a corrupt serial capture by dropping bytes.

When code, toolchain, board configuration, payload, or firmware image changes,
rerun the relevant workload and identify the exact artifacts used. Report host
preparation, VM evaluation, output packing, DMA completion, and physical LED
validation as separate boundaries. Do not describe an old retained capture as a
current measurement.

## Current verification commands

From the repository root, use the commands documented in `AGENTS.md`: format,
regenerate bindings, and run `pnpm check`. Criterion entry points and focused
workloads are listed in [Regression tracking](regression_tracking.md). Firmware
build, upload, checksum verification, profiling, and I2S commands live in
[ESP32 loading](esp32_loading.md) and `firmware/esp32/README.md`.
