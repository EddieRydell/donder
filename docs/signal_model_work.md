# Signal model implementation and measurement record

Scope: generator parameter automation, shared built-in operator semantics, and
spatial input-signal sampling. Maximum layer/output blending and 8-bit RGB remain
the accepted semantics. This is an in-progress record, not a completion claim.

## Requirements

- Generator automation must have explicit semantics for both child values and
  parameters that affect child count, targets, start times, and durations. Sampling
  every automated scalar only at the parent's start is insufficient. Preserve
  host-side import linking and preparation; do not add runtime source-name lookup.
- Scheduled frames, recursive frames, and recursive pixels must use the same
  built-in parameter binding, color operations, and temporal sampling rules.
  Preserve specialized loops only where measurements justify them.
- Operators must be able to choose an input pixel as well as an input time.
  Define indexing, bounds, fixture boundaries, and output-fragment behavior.
  Include the chosen pixel in sample-cache identity and compiler read reuse.
- Keep the current format a single development format; update its implementation
  and reject mismatched serialized data without compatibility branches.
- The user explicitly authorized updating tests and regression coverage. Run existing
  correctness checks and benchmark checksum assertions. Run cargo fmt followed by
  pnpm check for the completed implementation, and check the firmware workspace.

## Measurement setup

Fresh host baseline command: `pnpm bench:effect-vm:save`. Its Criterion baseline is
`effect-vm`, under `target/criterion`. Do not treat comparisons printed by its
initial quick pass against historical results as improvements from this work.
Capture the final `pnpm bench:effect-vm:compare` results after implementation.

The board was identified live on COM4 as ESP32 revision 3.1, 40 MHz crystal,
4 MB flash, MAC c8:2e:18:f1:5e:bc. Build the existing `dawn-esp32` profiling image
with `cargo +esp build --release --bin dawn-esp32 --locked` from `firmware/esp32`
after loading `export-esp.ps1`. Preserve exact baseline and final ELFs beside the
results under `target/signal-model-2026-09-06` before another build.

Full-flash backup attempts using espflash and esptool failed; no valid full backup
was obtained. A ROM-only 256-byte read at application offset 0x10000 succeeded.
It matches the preserved loader image header, including its embedded ELF hash.
The installed loader has not been overwritten.

The existing profiling collector requires 168 complete measurements, matching
host checksums, zero measured allocations, and return to the initial heap usage.
It measures evaluation and packing, not physical LEDs or Wi-Fi interference.
Report frame time, observed deadline violations, and memory with that boundary;
use the loader/I2S workflow for end-to-end output deadline measurements.

## Confirmed direction and target distinction

The user authorized generator automation to change generated behavior, including
child count and timing, not just child values. Generators must appear like normal
effects to the user. Actual authored target selection must remain fixed and must
not become an ordinary effect parameter. The existing type checker already rejects
target types in parameter declarations; preserve that distinction.

Interpretation to carry forward: a fixed parent target determines the available
fixtures/pixels, while generated behavior may distribute light differently within
that target (for example, chase steps and section counts). This does not authorize
selecting fixtures outside the parent target. Scope (PerFixture/WholeTarget) is a
separate execution setting, not an automatable parameter. The user subsequently
confirmed it should remain fixed; automating scope is outside this task.

The user explicitly authorized test modifications. The former test-permission and
fixed-versus-dynamic generator structure blockers are resolved. Dynamic generator
evaluation still requires implementation. Spatial sampling is implemented below
but still needs complete validation and final performance measurements.

## Generator preparation boundary requiring approval

Current source confirms that `CompiledEffect::generate_bound` executes arbitrary
parameter-dependent control flow and returns `GeneratedEffect` records containing
concrete start times, durations, targets, and parameter values. Preparation then
flattens them into `PreparedEffect` records. There is no retained expression for
how a generated child's existence or timing depends on automation.

The existing operator signal contract permits sampling arbitrary `SampleTime`
values, including between output frames and in nonmonotonic order. Expanding only
at automation knots or baking at the output frame rate would introduce a new,
inexact quantization rule and is not an implementation of the agreed normal-effect
contract. Sampling each child once at birth would also narrow that contract.

An exact implementation therefore needs a prepared time-dependent expansion plan:
the host resolves imports and numeric child slots and prepares the plan, while
portable evaluation computes the time-dependent generated behavior. This changes
the current fully-host-expanded playback boundary. User approval is being sought
before implementing that architectural change. It does not reopen the decisions
about fixed authored targets/scope or permission to update tests. No runtime
generator expansion or frame-rate baking has been introduced.

## Affected implementation areas

- `crates/dawn-language/src/dsl`: checking, compilation, optimization, and signal
  query representation.
- `crates/dawn-runtime/src/dsl`, `evaluation.rs`, `signal.rs`, `wire.rs`: VM,
  signal sampling/cache identity, shared native operators, and archive validation.
- `crates/dawn-elaboration/src/sequence`: generator expansion, graph preparation,
  and target coordinates.
- `crates/dawn-elaboration/src/output/fragment.rs`: preserve spatial dependencies
  when selecting controller outputs.
- `firmware/esp32`: existing workload generation and measurement integration as
  required by representation changes.
- `docs/sequence_as_code.md`: publish the resulting authoring/runtime contract.

## Current progress

- `dawn-runtime/src/operator.rs` now owns native operator binding, parameter
  bounds, unary/binary color semantics, and Echo time/weight generation. All three
  runtime traversals use these operations. Preparation and workspace sizing use
  the native operator's temporal/scratch requirements rather than their own lists.
- `cargo check -p dawn-runtime -p dawn-elaboration --locked --offline` passed;
  its unused-import warning was then removed. The existing
  `native_temporal_frames_match_scalar_sampling_through_nested_operators` test
  passed after the refactor. Full `pnpm check` subsequently passed for the current
  partial implementation; final firmware checks remain.
- The preserved baseline firmware is
  `target/signal-model-2026-09-06/baseline-dawn-esp32.elf`, SHA-256
  `ae2052f48cbd2220f8d234b63b8c6aa215e3fa74e419ca8cde07f7d8d0e6b08a`.
  The unchanged host benchmark executables are preserved in the same directory.
- Fresh full Criterion baseline means recorded so far: DSL suite 476.08 us,
  representative frames 960.61 us, dense 60-frame playback 7.3089 ms, and dense
  controller output 7.5545 ms. The full baseline process completed successfully.
  Some later microbenchmarks overlapped short compilation/check work; use a
  controlled rerun with the preserved baseline executable if they show a meaningful
  regression. Do not compare a modified build against itself as a baseline.
- The original installed I2S loader completed 90 playback windows (10,800 frames):
  zero missed deadlines, maximum evaluation 5,161 us, maximum total frame 7,894 us,
  and minimum reported free heap 77,976 bytes. Its 10 reference checksum checks
  passed with zero evaluation allocations. Capture:
  `target/signal-model-2026-09-06/baseline-i2s-playback.txt`.
  These are baseline results, not post-change validation.
- Spatial queries now support `source.at(time, local_pixel)` and
  `source.at_global(time, rig_pixel)` alongside current-pixel `source.at(time)`.
  The compiler tracks coordinate operands in register usage and read reuse;
  scalar cache identity includes the pixel and time. Out-of-range pixels are black.
  Frame caches retain full frames, so their identity remains input/time.
- Output fragments inspect reachable spatial reads. Local reads retain full
  selected color elements; global reads retain the full rig color domain. This
  preserves coordinates and unpatched upstream dependencies, at an explicit
  memory cost. In the starter fixture the full domain is 3,390 pixels, not the
  452-pixel baseline controller fragment.
- New DSL and rendering tests pass for coordinate domains, mutations, bounds,
  cached/scalar evaluation, seeks, split-output dependencies, and wire round trips.
  The archive marker is now 4; regenerate payloads with matching firmware.
- Full `pnpm check` passed after spatial implementation (session 19935, exit 0),
  including new local/global first-frame zero-allocation checks. An earlier run
  overlapped a source export edit and failed with stale compiled dependencies;
  the successful run used unchanged sources throughout.
- ESP32 `cargo +esp check --release --features i2s-output --bin loader --locked`
  passed for the new spatial bytecode (session 7275). No new firmware was flashed;
  this is compile validation, not a post-change board benchmark.
- Intermediate Criterion controller-output comparison: 8.3280 ms/60 frames versus
  the original 7.5545 ms baseline initially showed +10.24%. A back-to-back run of
  the preserved original executable measured 8.0690 ms (`--bench`, baseline
  `signal-control`); the changed build measured 8.2689 ms, +2.48% (about 3.3 us per
  frame), inside this benchmark's configured noise threshold. Retain this small
  measured cost for final verification; it does not justify speculative hot-path
  complexity. Sessions 99960, 16384, and 44745 exited successfully. This is not the
  final full benchmark comparison or post-change board validation.
- The existing native temporal equivalence test passed again after that cache
  groundwork. `cargo fmt --check` and full `pnpm check` passed (session 82465,
  exit 0), including frontend checks, 26 frontend tests, workspace tests, and
  warning-denying Clippy. Vite reported a nonfatal bundle-size warning. This is
  intermediate validation, not proof of the unfinished generator/spatial features.
  Final full benchmarks remain; the intermediate focused comparison is above.
- The user subsequently approved changing generator structure (but not authored
  target selection) and updating tests. No tests have yet been added or modified.

Full `pnpm check` sessions 80998 and 19935 have exited. Host
baseline session 11742, board capture session 14686, and backup session 3026
have all exited. The board still runs the original loader.
