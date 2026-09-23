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
