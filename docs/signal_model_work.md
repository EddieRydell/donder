# Signal model implementation and measurement record

Dawn expands generator structure on the host and evaluates retained child
parameters in its portable VM. This supersedes the earlier runtime-expansion
proposal. Authored targets and per-fixture/whole-target scope remain separate
fixed configuration. Maximum blending and 8-bit RGB semantics are unchanged.

## Fixed structure and live rendering parameters

`fixed param` is canonical declaration metadata for generators, sample effects,
and operators. Fixed values remain editable and require preparation; active
automation and live assignments to them are errors. Ordinary float, int, bool,
enum, and curve parameters retain their existing automation mappings.

The language staging pass follows assignments, arrays, indexing, branch merges,
and loop-carried dependencies. Live values cannot determine child existence,
count, timing, targets, effect selection, or fixed child parameters. Linked local,
imported, native, and unused emissions receive declaration-based type and
required-argument validation before expansion. Generator pixel reads are invalid.

Host specialization captures fixed locals at each emission and compiles retained
calculations with the existing typed compiler. Prepared bindings use numeric
slots and ordered parameter environments; imports and source identities stay out
of playback. Nested forwarding preserves the expression's lexical clock at the
exact requested SampleTime, including backward queries and children that outlive
parents. Child sampling retains its child-local clock. Constant-only projects
retain the static path.

Native MarkPulse and MarkChase use the same binding path. Their structural
parameters are fixed; rendering parameters and resource selection remain live.
MarkImpactBurst fixes its gradient collection because emptiness controls emission.
The editor derives fixed mode and automation eligibility from declarations,
rejects active fixed automation, and retains explicit detached bindings during
definition replacement. Typed edits, immutable history snapshots, and semantic
save/load ownership remain unchanged.

Runtime workspaces reserve parameters, VM registers, arrays, and automated curves.
Exact-time cache lanes share parent and child calculations across pixels and
recursive queries. Forwarded resource references are cleared before reusing a
lane so curve updates neither allocate through copy-on-write nor retain stale
crossing data. Controller fragments retain and remap ancestor environments and
child programs. The current archive marker is 5; archive CRC includes the retained
programs and binding metadata, while declaration/program hashes include fixed
metadata and emitted bytecode semantics. No compatibility path is provided.

## Signal queries

Operators support `source.at(time)`, `source.at(time, local_pixel)`, and
`source.at_global(time, rig_pixel)`. Negative/out-of-range pixels return black;
invalid times are errors. Scalar cache identity includes pixel and time. Local
queries preserve whole selected fixtures; global queries preserve the full rig
color domain even in a controller fragment. Output packing still includes only
selected ports. Native operator binding, color operations, and Echo time/weight
generation are shared by scheduled frames, recursive frames, and recursive pixels.

## Correctness coverage

Coverage includes fixed syntax/defaults, indirect structural dependencies, arrays,
loop captures, live branches, linked unused/imported emissions, nested clocks,
resource selection, automated curves and crossings, native generated children,
backward and alternating-time queries, malformed wire references, fragments, and
fresh/reused workspace parity. Allocation tests cover first and repeated playback
across the shared six generator workloads. Desktop coverage exercises editable
fixed values, rejected automation, definition replacement, detached rebinding,
undo/redo, and semantic save/load.

The shared workloads are forwarding, derived arithmetic, nesting, resource
selection, overlapping children, and automated curves. Each has an ordinary
sample-effect reference with identical output. Criterion compares ordinary live,
generated live, and generated constant forms at 200 and 800 pixels. The firmware
uses the same host-generated fixtures: 174 normal profiling records and 21
interrupted-PC fixtures (84 windows). Profiling is evaluation/packing evidence;
loader/I2S playback and external lights require their own evidence.

## Evidence and verification

Current work is recorded under `target/fixed-generator-completion-20260907/`.
The valid fresh `pnpm bench:effect-vm:save` completed before retained runtime
implementation, with unchanged sources and no concurrent builds. `baseline.log`,
`baseline-exit.txt`, `baseline-source.zip`, the preserved benchmark executables,
and `baseline-sha256.txt` identify it. The source archive SHA256 is
`0fdad7507c4529477344964956ddf71bf3bb099b06754fde28866bedb38dd3e2`.
New benchmark names have no pre-feature historical baseline; their initial saved
measurements must be identified separately from this baseline.

Earlier interrupted baseline attempts remain under
`target/fixed-generator-20260907-003402/`; they are invalid because they overlapped
source edits or builds. They are preserved for provenance, not used for acceptance.

The original loader's historical 10,800-frame I2S capture is
`target/signal-model-2026-09-06/baseline-i2s-playback.txt`: zero missed deadlines,
maximum evaluation 5,161 us, maximum frame 7,894 us, minimum reported free heap
77,976 bytes, matching reference checksums, and zero evaluation allocations.
These are baseline numbers. Exact prior firmware images are preserved in
`baseline-firmware/` within the current evidence directory. Earlier spatial-only
checks and Criterion comparisons are intermediate evidence, not final results.

Full checks, the Criterion comparison, and on-device profiling/I2S verification
are complete. Physical-light and oscilloscope validation were not performed.

`cargo fmt` followed by the full `pnpm check` passed (`check-final2.log`, exit 0),
including regenerated bindings, frontend checks/tests, Rust workspace tests, and
warning-denying Clippy. The firmware's combined `pc-profile,i2s-output` feature
check passed strict Clippy, and the normal profiler, PC profiler, and I2S loader
were built with the documented Xtensa toolchain and `--locked`. Six PC collector
unit tests passed. The interrupted-PC image is preserved but has not been
measured; the normal profiler supplies the device evaluation measurements below.

The board was reidentified on COM4 (ESP32 rev3.1, MAC c8:2e:18:f1:5e:bc) before
deployment. The normal profiling image SHA256 is
`29dac085e38b51a4e945e12511f2fc0c3f133a3cc71d9f08abf4102bd1154fc3`.
The first 19200-baud flash timed out; the identical retry succeeded. Both attempts
are preserved. `profile-final.txt` passed the collector with all 174 measurements,
matching host checksums, zero timed allocations, zero prepared-playback first-frame
allocations, and all 163,840 heap bytes recovered. Raw VM cold-start measurements
include workspace construction and are recorded separately from prepared playback.

| Live generator, 200 pixels | Mean / maximum frame (us) | First frame (us) | Retained bytes |
| --- | ---: | ---: | ---: |
| Forward | 742 / 747 | 1,379 | 7,552 |
| Derived | 766 / 781 | 1,629 | 8,200 |
| Nested | 788 / 804 | 1,629 | 8,784 |
| Resource selection | 238 / 265 | 1,415 | 9,844 |
| Overlap | 2,779 / 2,797 | 3,664 | 11,536 |
| Automated curve | 798 / 815 | 1,480 | 7,864 |

These measurements include the existing profiler's render/packing work and are
Wi-Fi-free. Larger stress cases can exceed a 120 Hz budget; the acceptance
workload is the representative 10,800-frame loader/I2S window, not every synthetic
stress case. All six live-generator cases remain below 8,333 us, including their
first frames.

The final I2S loader SHA256 is
`18f63fd6238f2e5857ce37b02e88774d3b17e3819222bb5928580d598264e1e4`.
The regenerated four-port starter archive is 25,361 bytes, 452 pixels, ten effects,
SHA256 `ca6581f73b33c817fedb45dc075e368ca24a19d00f303e7216192170c137144a`.
Three uploads succeeded, and all ten reference frames matched with zero evaluation
allocations. `i2s-final.txt` contains 92 complete playback windows; its first 90
consecutive windows are the comparable 10,800-frame acceptance window: zero
missed 120 Hz deadlines, maximum evaluation 5,300 us, maximum total frame 7,865 us,
and minimum reported free heap 81,208 bytes. Baseline maximum total was 7,894 us;
the final worst frame remains below the 8,333-us deadline. The normal profiler and
HTTP frame checks establish zero evaluation allocations; the networked I2S heap
measurement includes unrelated network activity.

This validates runtime execution, packing, and on-device I2S DMA completion.
External waveform voltage and physical-light behavior were not measured.

All six generator archives also passed the same loader/I2S path: 32 host-selected
frames per case matched with zero evaluation allocations, followed by nine
120-frame playback windows per case with zero missed deadlines. Captures are
`i2s-generator-{Forward,Derived,Nested,Resources,Overlap,Curve}.txt` in the evidence
directory. `loader-rejections-final.txt` confirms authorization, current-format,
size, CRC, interrupted-body, and concurrent-upload rejection while retaining a
working sequence. The board was left running the final loader and starter payload.

The completed representative Criterion comparisons against the valid pre-retained-runtime
baseline are below (mean times; dense workloads render 60 frames). Assertions
retain the original checksums and active-effect counts.

| Host workload | Baseline (ms) | Final (ms) | Change |
| --- | ---: | ---: | ---: |
| DSL suite, 4 x 512 pixels | 0.4910 | 0.4940 | +0.63% |
| Prepare starter | 0.5389 | 0.5203 | -3.46% |
| Representative frames | 0.9850 | 0.9826 | -0.24% |
| Dense playback, 60 frames | 7.5575 | 7.6614 | +1.38% |
| Dense cold playback, 60 frames | 7.5993 | 7.5135 | -1.13% |
| Dense controller output, 60 frames | 7.8732 | 7.8047 | -0.87% |

The dense playback increase is about 1.73 us per frame and within the configured
noise threshold; controller output and the representative device deadline result
do not show a corresponding regression. No hot-path complexity was added to chase
this small isolated difference. The full-suite comparison completed successfully in
`comparison.log`; new generator modes are compared with equivalent-output ordinary
samples, not claimed as improvements against a nonexistent pre-feature baseline.

`pnpm bench:effect-vm:compare` completed with exit 0, including its quick pass and
both full Criterion suites. No builds or source changes overlapped measurements.
The initial save of the 36 new generator names also completed with exit 0;
`new-generator-baselines.log` identifies these postimplementation measurements.
The full final mode comparison below reports mean microseconds per frame. Each
mode's output is checked against its equivalent ordinary reference before timing.

| Workload | Ordinary / live / constant, 200 pixels (us) | Ordinary / live / constant, 800 pixels (us) |
| --- | ---: | ---: |
| Forward | 5.494 / 5.600 / 5.466 | 21.846 / 22.019 / 21.831 |
| Derived | 5.483 / 5.679 / 5.525 | 21.376 / 21.917 / 21.453 |
| Nested | 5.481 / 5.685 / 5.468 | 21.938 / 22.151 / 21.743 |
| Resource selection | 0.850 / 1.249 / 0.658 | 2.318 / 2.746 / 2.154 |
| Overlap | 20.984 / 21.462 / 21.272 | 85.068 / 85.334 / 84.684 |
| Automated curve | 6.030 / 6.128 / 6.483 | 23.259 / 23.456 / 24.729 |

Live binding overhead against equivalent ordinary samples is 0.10-0.54 us in these
measurements, with no poor scaling from 200 to 800 pixels. The small resource case
has a larger percentage difference but costs only about 0.4 us more per frame.
Constant modes remove automation, so their inputs and curve-processing costs can
differ from the live modes; they are static-path references, not identical-input
speedup claims.

Two existing microbenchmarks were flagged as regressions: one-layer UniformFade
increased by about 0.14 us (to 0.794 us), and native mark-pulse playback by about
0.29 us (to 4.220 us). Those small absolute costs do not correspond to a
representative controller or device deadline regression. Their uncertainty and
tradeoff are retained here; no added complexity was justified to recover them.

Exact final host executables and `final-source.zip`, firmware ELFs, generated
archives and checksum sidecars are preserved in the evidence directory, with
`final-sha256.txt` identifying the artifacts. Earlier failed checks/flashes and
invalid baseline attempts remain preserved. No compatibility shims, temporary
implementation paths, or frontend development server were introduced.
