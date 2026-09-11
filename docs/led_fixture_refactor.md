# LED fixture simplification and performance

The refactor replaces the separate element tree and preview placements with
layout fixture instances. This document records the measurement baseline and
implementation and validation status.

## Model

- A fixture definition contains only ordered pixels, each with a stable ID,
  position, and diameter. Definitions cannot contain other definitions or groups.
- A layout owns independently addressable fixture instances and nested groups.
  Each instance references a reusable definition and has its own transform.
- Effects select entire layout instances or groups; effect cell ranges are removed.
- RGB/RGBW encoding and channel order belong to output routing. Fixture geometry
  is reusable independently of that choice.
- File references open the native editor tab; inline definitions use that same
  editor in a modal. No second representation or editor implementation.
- Profile fixtures, generic scalar/indexed outputs, control clips, profile
  encoders, shape primitives, and the old element/prop split are removed.
  Effect parameters and their automation, controllers, and LED routing remain.

## Current implementation boundary

The authoritative language project now owns layouts and fixture definitions.
Effects use layout fixture targets. The old language element tree, preview/prop
model, fixture profiles, control clips, and general patch graph are deleted.
Patches contain direct LED routes. Source identity/remapping, loading, saving,
and source inventories use the new types. The starter project is converted in
place, preserving its instance IDs and 30 outputs of 113 pixels each.

Prepared definition pixels are shared between instances; layout group targets resolve
to contiguous ranges of instances. Runtime output packing now reads the signal
color buffer directly. Generic control evaluation, fixture behavior rules, and
intermediate element/patch-value storage are deleted. Selected-output compaction
retains original effect coordinates. Runtime/preparation APIs use fixture names.

The desktop uses a layout tree and a flat pixel editor. File definitions open in
tabs; inline definitions use the same editor in a modal. Layout creation can
reuse a definition or create an inline one without a separate setup step.
Pixel edits commit directly through the normal history; no draft or Apply mode.

The performance measurements below are from the earlier LED refactor, before
the flat-definition simplification. They are historical results, not fresh
performance acceptance for the current editor and authoring format.

## Intermediate host comparison

The core refactor was compared with the preserved baseline before desktop GUI
migration. These are provisional measurements, not final goal acceptance.

| Workload | Before central estimate | Core refactor |
| --- | ---: | ---: |
| `prepare_starter` | 512.58 us | 168.10 us |
| `render_representative_frames` | 957.29 us | 972.46 us |
| `render_playback_dense_60_frames` | 7.3467 ms | 7.5701 ms |
| `controller_output_dense_60_frames` | 8.1767 ms | 7.6318 ms |
| VM `effect-vm` | 474.60 us | 487.69 us |

Criterion reports preparation and controller-output improvements. The small
signal-rendering increases are within the existing 5% noise threshold; they
remain visible here rather than being described as universal improvement.
Log: `target/led-model-core-after-render.log`.
The VM change is also within Criterion's 5% noise threshold; its log is
`target/led-model-core-after-vm.log`.

The device suite now has 170 cases. Four `gamma_raw` cases exercised the deleted
component-filter implementation and were removed. The production `gamma_lookup`
cases retain their golden-output checks.

## Intermediate ESP32 comparison

The 170-case capture completed successfully on the same ESP32, at 240 MHz,
one core, Wi-Fi off, GPIO untouched. All golden checksums passed, timed
allocations and prepared first-frame allocations were zero, and the complete
163,840-byte heap was recovered. These measurements still exclude scheduled
output and physical LED acceptance.

All 134 prepared-playback cases had lower mean frame times. The 36 raw VM
cases were effectively unchanged (worst increase: 1 us, 0.06%). Prepared
retained memory fell by 1,992 bytes at 200 pixels, 3,792 at 400, 7,392 at 800,
and 14,592 at 1,600. The median mean-time reduction among the 36 plain show
cases was 16.52%. Nested operator workloads improved more; these are individual
workload comparisons, not an aggregate speedup claim.

| Workload | Mean before / after | p95 before / after | Retained bytes before / after |
| --- | ---: | ---: | ---: |
| ScanSweep, show, 200 pixels | 6.246 / 5.026 ms | 6.248 / 5.028 ms | 9,700 / 7,708 |
| ScanSweep, show, 800 pixels | 24.908 / 20.072 ms | 24.915 / 20.081 ms | 30,100 / 22,708 |
| PixelRamp, four layers, 1,600 pixels | 19.851 / 19.665 ms | 19.852 / 19.666 ms | 69,952 / 55,360 |
| GeneratorOverlap, 200 pixels | 2.724 / 2.675 ms | 2.730 / 2.681 ms | 11,536 / 9,544 |

Capture: `target/led-model-core-after-esp32.txt`. Preserved firmware:
`target/led-model-core-after-esp32.elf`, SHA-256
`3dfbbf9c8634f9b48c7dfd145e590e249725696582636703d8f8936ece0627a1`.
All baseline cases match except the four intentionally removed `gamma_raw`
cases; no records were combined across runs.

## Before measurements — 2026-09-09

Host Criterion baseline `led-model-before` in `target/criterion`:

| Workload | Central estimate | Reported interval |
| --- | ---: | ---: |
| `prepare_starter` | 512.58 us | 507.91–517.70 us |
| `render_representative_frames` | 957.29 us | 948.46–966.45 us |
| `render_playback_dense_60_frames` | 7.3467 ms / 60 frames | 7.3029–7.3931 ms |
| `controller_output_dense_60_frames` | 8.1767 ms / 60 frames | 8.0712–8.2874 ms |

The VM `effect-vm` baseline completed at 474.60 us (470.83–478.31 us).
The subsequent all-workload render collection was interrupted; the four render
workloads above were collected separately and completed successfully.
Raw logs: `target/led-model-before-host.log` and
`target/led-model-before-render.log`.

ESP32 baseline: revision v3.1, 240 MHz, one core, Wi-Fi off, GPIO untouched.
The preserved ELF is `target/led-model-before-esp32.elf`, SHA-256
`2ca656fdce96ff24738fe865f82c702aceef23231d5b02d8207e4fe84a7454db`.
Flash completed on COM4. `capture.py` successfully validated all 174 records,
zero timed allocations, zero checksum mismatches, zero prepared first-frame
allocations, and full recovery of the 163,840-byte heap.

| ESP32 workload | Mean | p95 | Max | Retained bytes |
| --- | ---: | ---: | ---: | ---: |
| ScanSweep, show, 200 pixels | 6.246 ms | 6.248 ms | 6.280 ms | 9,700 |
| ScanSweep, show, 800 pixels | 24.908 ms | 24.915 ms | 24.938 ms | 30,100 |
| PixelRamp, four layers, 1,600 pixels | 19.851 ms | 19.852 ms | 19.855 ms | 69,952 |
| GeneratorOverlap, 200 pixels | 2.724 ms | 2.730 ms | 2.733 ms | 11,536 |

The accepted capture is `target/led-model-before-esp32-retry.txt`.
The first capture (`target/led-model-before-esp32.txt`) contains a corrupt serial
record and is rejected. Do not merge its records into the accepted run.

These firmware measurements cover VM/prepared evaluation, not LED electrical
output or scheduled missed-frame counts. `setup_us` measures on-device prepared
data construction, not host elaboration. After migration, repeat the same
Criterion and firmware workloads and perform representative output/deadline
profiling before claiming end-to-end runtime parity. Intermediate after
measurements are recorded above.

## Final host comparison

The controlled representative rerun used the preserved Criterion baselines.
No other builds or benchmark processes ran during the timing windows.

| Workload | Before slope estimate | Final slope estimate | Criterion mean change |
| --- | ---: | ---: | ---: |
| prepare_starter | 512.58 us | 173.25 us | -65.86% |
| render_representative_frames | 957.29 us | 983.22 us | +2.82% |
| render_playback_dense_60_frames | 7.3467 ms | 7.5613 ms | +2.92% |
| controller_output_dense_60_frames | 8.1767 ms | 7.5572 ms | -7.58% |
| dsl_effect_suite_4x512_pixels | 474.60 us | 471.21 us | +1.38% |

Criterion's displayed time uses a slope estimate; its relative-change test uses
sample means, which explains the different direction for the small VM change.
The signal-only and VM changes are below the repository's existing 5% noise
threshold. Signal-only rendering measured about 0.215 ms more over 60 frames;
this is reported, not presented as an improvement. Controller output and
preparation improved significantly. No significant controller-playback
regression was observed.

Logs: target/led-model-final-render.log and target/led-model-final-vm.log.

## Scheduled ESP32 playback

The output-enabled loader was flashed to the same ESP32 rev 3.1 on COM4.
The four-port starter archive contains 452 pixels and is 24,839 bytes. Three
HTTP uploads succeeded; ten nonsequential sample-frame checksums matched the
host, with zero evaluation allocations. During a 90-second capture, 88 complete
120-frame windows covered 10,560 frames and reported zero missed deadlines.

- Median window evaluation average: 1.733 ms; maximum window average: 4.715 ms.
- Worst individual evaluation: 5.113 ms.
- Median window total average: 6.311 ms; maximum window average: 7.251 ms.
- Worst individual total: 7.659 ms, below the 8.333 ms budget at 120 Hz.
- Steady free heap: 83,848 bytes; observed minimum: 82,148 bytes.
- Median I2S encoding average: 2.471 ms.

Evidence: target/led-model-i2s-after.txt.
Measured loader ELF SHA256:
2d278d7163e7d721814bf8496149ce6541fb7e4a097b64cea6a822777bc74794.
Archive SHA256:
f2e261fab208f446bf6f524aed5bbeb084c0c15c574c24966d9cade42e94a6b4.

There was no freshly captured pre-refactor scheduled-output baseline. The
2026-09-06 historical run also had zero misses (13,080 frames), median total
6.348 ms, worst total 7.939 ms, and steady free heap 81,376 bytes. Its window
alignment and firmware differ, so these are context, not a controlled speedup
claim. The 170-case compute suite above provides the paired before/after device
comparison. This run measures evaluation, encoding, and DMA completion, not
external waveform or physical LED behavior.

## Final packaged-firmware capture

After the final DSL field-name cleanup and bundled-image regeneration, the
current loader was flashed and the same three uploads, ten checksum samples,
and 90-second playback capture were repeated. All checksum samples passed
with zero evaluation allocations.

The capture contains 89 complete windows / 10,680 frames. **Four deadlines
were missed in the first window following upload/checksum activity**, whose
worst total frame was 42.569 ms. That result is retained. Flash/network/verification
activity is a plausible contributor, but this run does not isolate its cause;
it is not evidence of uninterrupted playback through uploads.

The following 88 windows / 10,560 frames had zero misses:
- Median window evaluation average: 1.733 ms; highest: 4.357 ms.
- Worst individual evaluation: 5.203 ms.
- Median window total average: 6.310 ms; highest: 7.096 ms.
- Worst individual total: 7.843 ms, below the 8.333 ms budget.
- Median encoding average: 2.470 ms.
- Reported free heap remained 83,784 bytes throughout the recorded windows.

This establishes the observed steady playback result and exposes the initial
transition misses. There is no paired pre-refactor upload-transition capture
from which to claim an improvement or regression in those misses.

Evidence: target/led-model-i2s-final.txt.
Final measured loader ELF SHA256:
3b2ac2c2343b41262140e2152ef735a9519d08dbd904fd88f163042aafa45cce.
The archive hash is unchanged from the preceding capture.

Host allocation checks also pass after the final model changes. The layered
starter workspace uses 42,095 bytes for the full setup and 1,867 bytes for its
first selected port; the empty-effect sequence uses 10,170 and 339 bytes.
These are allocator-requested workspace bytes on this laptop, excluding prepared
data, output buffers, allocator overhead, and stack. Prepared playback, selected
archives, and generator automation cases perform zero timed allocations.
Evidence: target/led-model-host-allocations.log and the full test gate.
