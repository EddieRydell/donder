# Effect VM Performance

This is a historical optimization notebook. Current fixture-model measurements
are in the [LED refactor report](led_fixture_refactor.md); earlier embedded
measurements are recorded in the [execution audit](execution_audit_2026-09-06.md).

Dawn uses Criterion for Effect DSL VM and real-project render benchmarks. Timing deltas are
advisory; benchmark assertions fail only when the VM or renderer changes output.

## Commands

Run the full Effect VM benchmark set:

```powershell
pnpm bench:effect-vm
```

Run a faster local smoke pass:

```powershell
pnpm bench:effect-vm:quick
```

The quick pass only checks that every benchmark runs. Its reduced sample count is not reliable
enough to classify small performance changes.

Save a baseline before optimization work:

```powershell
pnpm bench:effect-vm:save
```

Compare the current working tree against the saved baseline:

```powershell
pnpm bench:effect-vm:compare
```

Criterion stores baselines and reports under `target/criterion`; do not commit them.

The full harness warms each workload for three seconds, measures for five seconds, and treats changes
below five percent as environmental noise. Longer sequential runs caused laptop power and thermal
state to drift enough to make later renderer cases slower without code changes. On Windows the
harness also pins the benchmark thread to a fixed nonzero logical CPU to avoid migrations between
unlike cores. The pnpm full/save/compare commands launch one quick untimed pass first so newly linked
executables finish one-time work before measurement.

## Focused Runs

Run one VM benchmark by name:

```powershell
cargo bench -p dawn-language --bench effect_vm_bench -- dsl_effect_suite
```

Run the representative render batch:

```powershell
cargo bench -p dawn-elaboration --bench render_bench -- render_representative_frames
```

## Coverage

`crates/dawn-language/benches/effect_vm_bench.rs` measures direct VM paths through the public Effect
DSL APIs:

- `sample_bound`

The effect suite evaluates four representative DSL effects across 512 pixel contexts each, and the
operator measurement evaluates 512 signal samples. Together they cover curves, branches, section
position, smoothstep, enum comparisons, seeded random paths, trigonometry, HSV color generation,
and Signal input sampling. Sub-microsecond single-call benchmarks, native helpers unchanged by the
VM, and allocation-heavy generator timing were removed because power-state and allocator noise
repeatedly produced false regressions on unchanged code.

`crates/dawn-elaboration/benches/render_bench.rs` measures the real
`examples/starter/project.dawn` renderer:

- renderer preparation
- one batch of seven representative sparse and dense frames
- warmed and cold 60-frame playback batches
- warmed controller-output batches through patch evaluation and port bytes
- frame checksums and active effect counts before timing begins

Update committed checksums only when a renderer or DSL behavior change is intentional.

## Runtime Workspace Pass (2026-09-04)

Graph frame slots now occupy one fixed boxed color slice instead of a vector of independently
growable vectors. Evaluation writes into prepared slot ranges and restores the backing storage
even when sampling returns an error. The desktop convenience API allocates its additional output
frame only when used; direct controller evaluation no longer reserves that unused frame.

Repeated effect samples use the existing dense pixel index for lookup instead of scanning cached
keys. Each entry records the pixel count too, so differently sized fixtures cannot share a cached
color incorrectly. Differently sized fixtures can evict each other's entries and require resampling;
this is not a guarantee of one evaluation per distinct context. Entries are 8 bytes, and the table
is sized to the largest pixel count among effects that reuse samples. Previously it reserved one
12-byte entry on 32-bit targets (24 bytes on the desktop) per pixel of the largest such target.
Frame-only graphs also omit the unused 1,024-entry recursive signal cache.

Full warmed Criterion runs before and after this pass, on the same desktop:

| Workload | Before | After |
| --- | ---: | ---: |
| Seven representative frames | 4.471 ms | 4.522 ms |
| Dense playback, 60 frames | 13.344 ms | 13.276 ms |
| Cold-workspace playback, 60 frames | 13.192 ms | 13.077 ms |
| Controller output, 60 frames | 15.673 ms | 15.661 ms |

Criterion classified all four playback changes as no measurable change. This pass reduces reserved
memory and buffer bookkeeping; it does not establish a playback speedup. These are desktop results,
not ESP32 measurements. The two runtime source files together lost 21 lines, including blank lines
and comments. Existing controller allocation assertions and behavioral checksums still pass.

Calculated VM arrays can still allocate, reference-counted resources remain, and the final compact
transfer artifact is unfinished. This workspace pass does not make arbitrary programs heapless.

## Prepared Program Table (2026-09-04)

`PreparedSequence::programs` now owns a frozen table of bytecode programs. Sample effects and DSL
operators hold 32-bit indices into that table. Elaboration interns effect definitions and repeated
operator definitions, so graph nodes no longer each own a boxed copy of custom-operator bytecode.
The playback table has no reference-counted program ownership. Constants and other resources
inside those programs still need conversion to compact resources; this is not yet a flat artifact.

Desktop raster preparation uses the same program indices and VM, but keeps host-side shared
ownership of its program entries so preparing multiple clip previews does not duplicate their
bytecode. The host elaboration cache is discarded when freezing a playback sequence.

The final warmed renderer comparison against the immediately preceding workspace version was:

| Workload | Before | After |
| --- | ---: | ---: |
| Seven representative frames | 4.587 ms | 4.363 ms |
| Dense playback, 60 frames | 13.703 ms | 12.959 ms |
| Cold-workspace playback, 60 frames | 13.277 ms | 13.314 ms |
| Controller output, 60 frames | 15.712 ms | 15.713 ms |

Criterion classified the first two changes as within the noise threshold and the last two as no
measurable change. No robust playback speedup or regression is established. The standalone VM
comparison also stayed within the noise threshold. Full `pnpm check`, existing allocation and
checksum assertions, and the `riscv32imc-unknown-none-elf` no-default-features build passed.

This pass adds 69 Rust source lines across eight files, including comments and blank lines:
14 in the runtime and 55 in elaboration/desktop-raster preparation. The growth implements explicit
program indexing, deduplication, and table ownership rather than another execution engine. It removes
reference-counted playback program handles and duplicated operator programs, not the remaining
target/curve/gradient/array resource ownership. Retained-byte savings have not been measured yet.

## Frozen Target Pool (2026-09-04)

Prepared effects and the signal graph now reference targets by 32-bit index. Each `PreparedTarget`
contains a 32-bit pixel range and a precomputed sample-cache width; all target pixels live in one
boxed slice. The descriptor is 12 bytes and each pixel remains 16 bytes. This replaces per-target
reference-counted playback ownership with two plain arrays. Overlapping but nonidentical layouts
are not yet stored as shared subranges.

Elaboration interns identical target contents, including exact pixel-fraction bits and local pixel
indices/counts, not merely output addresses. Generated and ordinary targets use one preparation
cache in `sequence/targets.rs`; the nested generator-specific cache wrapper was removed. Host
preview batches still share their source layouts. The per-effect `reuse_samples` flag was removed:
workspace sizing reads target metadata, and each effect clears only its own required cache width.

This pass adds 57 Rust source lines across eight files (10 in the runtime, 47 in elaboration).
The additions implement target interning, checked pool construction, and explicit range/cache
metadata. Full `pnpm check`, the existing allocation/checksum assertions, and the 32-bit
no-default-features build pass. Actual retained-memory totals remain to be instrumented; do not
interpret the record sizes above as a measured whole-show footprint.

Timing remains unresolved rather than a demonstrated speed improvement. The saved pre-change
baseline measured 539 us for the VM suite, 4.059 ms for representative frames, 12.291 ms for dense
60-frame playback, and 14.608 ms for controller output. One completed target-pool run measured
576 us, 4.301 ms, 12.648 ms, and 15.087 ms respectively. A later full verification run measured
602 us, 4.469 ms, 13.626 ms, and 15.492 ms. Criterion flagged several regressions, including the
standalone VM suite, which does not use prepared target storage. Preparation rose from 475 us to
545 us; content interning and copying the flat pool also introduce genuine elaboration work.

As a control, the exact pre-target-pool source was temporarily restored and warmed again, then
the target-pool work was restored before final verification. That unchanged source measured
559 us for the VM suite and 4.304 ms for representative frames, reproducing the representative
regression flag against its own earlier baseline. This establishes baseline drift, not that every
reported regression is environmental. Stable attribution and benchmark reproducibility remain
unfinished parts of the goal; the later measurements must not be hidden by the earlier faster run.

## Patch Buffer Lifetimes (2026-09-04)

Elaboration now resolves fan-out ports to their input slot instead of retaining copy instructions
and independent output buffers. It records each logical value's last reader and assigns reusable
physical slots of the same type and width. Inputs remain live through the filter invocation, so
input and output never alias. These lifetime tables exist only during elaboration; playback uses
the remapped indices directly.

Patch topological order now starts in declared node order rather than randomized hash-map order,
and newly ready consumers run before another independent branch. This preserves dependencies
while shortening intermediate lifetimes. It removes one source of execution-order variability;
it does not resolve the separate machine-level benchmark drift.

Temporary preparation diagnostics, removed after measurement, reported 120 logical value buffers
reduced to four physical buffers for each starter controller preparation. Those four contain 113
RGB colors, two sets of 339 f32 components, and 339 output bytes: 3,390 payload bytes instead of
101,700. This excludes vector/descriptor metadata, allocator overhead, retained prepared programs,
element states, and the final controller-port buffers; it is not a whole-show heap measurement.

The pass adds 39 Rust source lines including comments and blank lines: 36 in elaboration and three
in patch ordering. The additional code computes lifetimes once instead of retaining an independent
allocation per intermediate. Full `pnpm check`, controller checksum/allocation assertions, and the
32-bit no-default-features runtime check pass. Existing starter coverage does not contain fan-out;
its aliasing behavior is supported by the immutable filter-input contract, not a dedicated test.
Patch execution still depends on host authoring types, and calculated VM arrays still allocate.

The final full Criterion comparison measured 619.61 us for the standalone VM, 551.58 us for
preparation, 4.503 ms for representative frames, 13.499 ms for dense 60-frame playback, 13.409 ms
for cold playback, and 15.998 ms for controller output. Criterion classified every change as
either no measurable change or within the configured noise threshold. The clean controller
baseline was 16.351 ms: the exact pre-pass sources were temporarily restored, measured without
concurrent checks, and then the verified final sources were restored and compared. This replaced
an earlier 15.706 ms controller baseline that briefly overlapped compilation. No robust speedup
is claimed; the buffer reduction is the demonstrated benefit.

## Portable Fixture Encoding (2026-09-04)

`dawn-runtime::fixture::FixtureProgram` encodes prepared fixture functions and channel instructions
into a caller-owned byte slice. Function/channel tables contain numeric IDs, curves, indexed DMX
ranges, and precomputed fine-channel flags, not profile identities, names, tags, or maps. Indexed
entries are sorted during preparation and searched by ID. Prepared patch evaluation constructs the
program once; direct authoring-filter evaluation compiles before invoking the same encoder. The
old profile-driven encoder was removed rather than retained as a second execution path.

Fixture value types and dimming/quantization math now belong to the portable runtime and are
re-exported by the authoring crate. The existing `libm` dependency supplies f32 power and rounding;
no dependency was added. Coarse-only channel behavior, including its existing channel-curve bypass,
is intentionally retained rather than changed as part of this refactor.

This is not the complete portable output path: fixture state still uses an ID/value list, controls
still retain host profile data, other filters still use authoring definitions, and custom curves
still own point vectors. The encoding loop itself uses caller-owned storage without allocation,
but the existing allocation test exercises starter RGB output, not fixture encoding. Approval for
focused fixture tests has been requested under the repository's explicit-test-approval rule.

Full `pnpm check`, existing starter checksum/allocation assertions, and the 32-bit no-default-features
runtime check pass. The pass adds 188 Rust lines across six files: 231 runtime lines, 57 fewer patch
language lines, 45 fewer profile language lines, and 59 elaboration lines. Growth comes from the
compiled data representation, lowering, and prepared-patch integration. No reduction in total
retained fixture memory is claimed while host profiles and the compiled representation coexist.

The completed full comparison measured 623.06 us for the VM, 558.47 us for preparation, 4.533 ms
for representative frames, 13.275 ms for dense playback, 13.547 ms for cold playback, and 16.599 ms
for controller output. All but controller output were classified as no measurable change;
controller output increased about 2% from its 16.267 ms baseline, within the configured noise
threshold. This suite does not exercise fixture-profile encoding or gamma curves, so these numbers
do not establish the new encoder's performance or cross-platform rounding equivalence.

## Prepared Fixture Behaviors and Buffer Layouts (2026-09-04)

Fixture behaviors now lower to numeric element/function bindings and a portable operation:
color mixing, an on/off value switch, or color-wheel lookup. Shutter, prism, and dimmer rules share
the switch operation. Lowering preserves function/rule order, and evaluation still skips functions
with active explicit controls. The old per-frame profile/rule interpreter was removed.

Patch values and their allocation layouts now live in `dawn-runtime::patch`. Layouts retain 32-bit
widths and fixture-function capacities, not profile identities. Elaboration resolves capacities
once; workspace creation no longer needs the authoring profile store. Consequently
`PreparedSequenceOutput` no longer retains `FixtureProfileStore`. Fixture element templates retain
their preallocated state capacity, which workspace construction restores after cloning.

Direct authoring `evaluate_filter` still supports all filters, compiling fixture encoding before
invoking the shared runtime encoder. `evaluate_filter_into` now handles non-fixture filters without
a profile-store argument; prepared fixture playback invokes `FixtureProgram` directly. There is no
empty-store fallback or second fixture encoder.

Full `pnpm check`, starter checksum/allocation assertions, and the 32-bit no-default-features check
pass. Dedicated fixture behavior/encoding tests remain pending explicit approval; starter RGB
checks do not establish that coverage. Non-fixture filter definitions and some output identities
remain host-owned, and calculated VM arrays still allocate. This is not yet a complete embedded
output artifact. Retained-memory totals for a fixture-bearing show have not been measured.

The pass adds 121 Rust lines across seven files: 79 in the runtime, nine in language patch handling,
40 in control lowering, seven in the output session, and 14 fewer in patch execution. The growth
implements the prepared representation and removes dependence on a retained authoring store rather
than adding another playback engine.

The first full comparison for this pass flagged controller output at 18.291 ms against 16.687 ms
(about +9.6%). The implementation was retained. An unchanged focused rerun measured 16.860 ms;
a temporary old-source control measured 16.739 ms, after which all new sources were restored and
verified against their saved contents. Another focused new-source run measured 17.592 ms with a
burst of high samples: one consecutive group of 20 averaged 21.36 ms, versus roughly 16-17.5 ms
for the other groups. A complete new-source suite rerun then measured 16.123 ms for controller
output. This does not establish a persistent code regression, nor identify the transient cause.

A 30-second diagnostic run with Windows counters measured 16.549 ms. During the 22-second observed
interval, the reported performance limit stayed at 100%, the processor-performance counter ranged
roughly 168-178%, DPC time reported zero, and interrupt time was mostly zero with a few readings
near 1.55%. No large burst reproduced. Monitoring can perturb measurements, and these observations
cannot explain an earlier unobserved event. A named CPU-light ETW recording failed to start because
Windows could not enable profiling policy (0xc5585011). No recording or ETL was left behind. Root
cause and benchmark reproducibility remain open; regression flags prompt investigation, not an
automatic rollback or deletion of the benchmark.

## Shared Fixture Behavior Table (2026-09-04)

Compiled behavior rules, including color-wheel entry arrays, are now stored once per profile in
each prepared output. A fixture binding contains its numeric element index and a 32-bit range into
the shared rule table. Empty rule blocks produce no bindings. Profile interning happens only during
elaboration; its map and profile IDs are discarded. Rule order and explicit-control precedence are
unchanged. `FixtureBehaviors` is a portable runtime data type, not another evaluator.

This fixes the previous pass's per-fixture duplication of rule payloads. Encoding programs are still
owned per patch encoding step; sharing those remains separate work. Whole-show retained bytes and
fixture-heavy performance have not been measured, and dedicated fixture coverage remains pending
test approval. The change adds 24 Rust lines: six in the runtime table declaration, 17 in lowering
and evaluation, and one in the output session. The added code interns profiles and checks range
limits rather than retaining duplicate color-wheel tables or using reference counting.

Full `pnpm check` and `cargo check -p dawn-runtime --no-default-features --target
riscv32imc-unknown-none-elf` passed. The full saved-baseline comparison measured VM evaluation
at 614.95 us, preparation at 558.17 us, representative playback at 4.5856 ms, dense playback
at 13.294 ms, cold-workspace playback at 13.269 ms, and controller output at 16.206 ms.
Criterion classified the first three increases as within its noise threshold and the last
three as no measurable change. Controller output's baseline was 16.172 ms. These starter RGB
measurements do not exercise the shared fixture behavior tables or close the earlier transient
slowdown investigation.

## Shared Fixture Encoding Programs (2026-09-04)

Prepared patch encoding steps now hold a checked 32-bit program index instead of owning a boxed
program. Preparation interns programs by `(profile, slot_count)` and freezes them into one boxed
table; its temporary lookup map is discarded. Slot count is part of the key because it controls
each fixture's output stride. Fixture count is not: one program encodes any number of states.
Function, channel, entry, and curve payloads are consequently stored once per distinct key rather
than once per encoding step. Playback uses a direct table lookup, with no reference counting.

This pass adds 17 Rust lines in `output/patch.rs` for the shared table and checked host-side
interning. It removes duplicate retained payloads, not the encoding algorithm. An empty table
adds a boxed-slice field to patches without fixtures, so this is not a claim that every patch
uses fewer bytes. Whole-show memory savings and fixture-heavy timing remain unmeasured.

Full `pnpm check`, including existing controller allocation and checksum assertions, passed.
The `riscv32imc-unknown-none-elf` no-default-features runtime check also passed. No new tests were
added; dedicated fixture-program sharing coverage remains pending approval.

The full saved-baseline comparison measured VM evaluation at 618.07 us, preparation at
560.38 us, representative playback at 4.4679 ms, dense playback at 13.466 ms, cold-workspace
playback at 13.022 ms, and controller output at 16.035 ms. Criterion classified the first four
increases as within its noise threshold and the last two as no measurable change. Controller
output's baseline was 16.061 ms. This starter RGB suite does not measure fixture sharing's benefit
and does not resolve the previously observed transient slowdown.

## Portable Prepared Filters (2026-09-04)

Non-fixture filter execution now lives in `dawn-runtime::patch::PreparedFilter`, alongside its
value buffers. Prepared output no longer retains `FilterDefinition`: host preparation converts
widths to checked u32 values, freezes reorder tables, sorts indexed mappings into numeric pairs,
and flattens discrete-color levels into emitter order. Discrete tables retain colors and numeric
levels, not emitter names, IDs, or maps. Lookup preserves the first matching color and the existing
zero level for an omitted emitter. Indexed lookup uses binary search instead of hashing.

The old language evaluator and unused discrete-map helper were removed. Desktop one-shot
`evaluate_filter` prepares then invokes the same runtime implementation; playback prepares once.
Fan-out remains eliminated in prepared patch lowering, while the shared kernel supports the
one-shot authoring operation. Fixture encoding still uses its separately prepared program.

The pass adds 173 Rust lines across three files: 236 in the runtime, 66 fewer in language, and
three in output elaboration. This growth pays for an explicit metadata-free representation,
checked conversion, and host error conversion; the evaluation algorithms were moved rather
than duplicated. There is no new dependency or second execution engine. Output buffers still
use preallocated Vec storage; arbitrary callers must provision sufficient capacity to avoid
growth. Whole-show retained memory and discrete/indexed-heavy performance remain unmeasured.

Full `pnpm check` and the no-default-features `riscv32imc-unknown-none-elf` runtime check passed.
Existing RGBW/quantization, starter checksum, and starter controller-allocation tests passed.
There are no new tests for discrete/indexed preparation; adding that coverage requires approval.

The full comparison measured VM evaluation at 625.64 us, preparation at 560.32 us,
representative playback at 4.5063 ms, dense playback at 13.161 ms, cold-workspace playback at
13.291 ms, and controller output at 16.401 ms against a 16.383 ms baseline. Criterion classified
the VM increase and dense-playback decrease as within its noise threshold, and all other changes
as no measurable change. The starter workload does not establish discrete/indexed performance.
The earlier transient slowdown remains unresolved; no implementation was reverted over a flag.

## Portable Element State and Patch Sources (2026-09-04)

`RenderedElementState` and its numeric `ElementNodeId` now live in `dawn-runtime::element`.
Rendered state no longer retains color capabilities (including discrete emitter names/maps) or
fixture profile identities. Preview consumes only node IDs and values; profile and capability
data remain in authoring/preparation. Existing preview color conversion uses the shared portable
quantizer for grayscale. The convenience `preview_colors` method still allocates its returned
buffer; controller sampling does not call it.

Patch-source copying now lives in `dawn-runtime::patch::PatchSource`, with checked 32-bit
element/cell addresses. The redundant source-kind tag was removed: the prepared destination
buffer determines the operation. Fixture selections are checked against their declared profile
during preparation, replacing per-frame profile identity comparisons. Invalid profile selections
now fail preparation rather than first playback. Runtime source failures use a structured error;
the host output boundary formats it.

This pass removes one Rust line overall across eight files, including the new runtime element
module. Runtime code moved from elaboration rather than adding another renderer. It removes
retained metadata and its cloning, but whole-show retained bytes have not been measured. Output
session/controller-frame orchestration and control evaluation still need portability work.

That pass's full check and 32-bit no-default-features check passed. Controller output measured
16.425 ms against 16.627 ms, classified as no measurable change. Preparation initially flagged
578.02 us against 542.86 us; an unchanged focused rerun measured 538.43 us with no measurable
change. The measured preparation function calls `elaborate_sequence`, not output-patch lowering.
The slowdown's cause is unproven and the refactor was retained.

## Portable Patch Executor and Approved Coverage (2026-09-04)

The prepared patch, its workspace, and its executor now live in `dawn-runtime::patch`.
Elaboration retains graph validation, fixture-program interning, index resolution, and buffer
liveness planning. Prepared step indices and destination ranges use u32. The executor accepts
writable byte buffers via standard `AsMut<[u8]>`, so raw arrays/slices and desktop controller frames
use the same implementation without an extra copy or a custom backend interface. Network and
pin transmission remain outside it. Runtime errors are structured; only the host formats them.

Elaboration now resolves identity component reorders as aliases, like fan-out, before buffer
planning. Their copy instructions and intermediate values do not reach runtime. Nonidentity
reorders remain. This is not yet a single portable prepared-show artifact: control evaluation and
session orchestration still live in elaboration, and preview still reads logical element state.
Moving preview to shared patched output remains required by the agreed design.

With explicit approval, focused tests were added for identity/nonidentity lowering and actual
controller output, discrete mapping order and missing levels, sorted indexed mappings, fixture
coarse/fine bytes, gamma, indexed entry ranges, RGBW channels, ignored slots, fixture stride, and
error cases. One allocation-counted test executes the prepared source/filter/fixture/sink path
100 times, including its first evaluation, with zero allocations.

The tests exposed a fixture bug: the encoder subtracted white from RGB channels even for RGB-only
fixtures. Input [128, 64, 32] incorrectly encoded as [96, 32, 0]. Preparation now resolves whether
the function uses RGBW and stores that flag; RGB skips white extraction and preserves its channels.
Both RGB and RGBW cases pass. This intentionally corrects fixture behavior rather than preserving
the erroneous RGB result.

The pass adds 39 implementation lines, plus 442 test/declaration lines (481 total). The implementation
growth covers portable errors, checked numeric indices, identity elimination, buffer access, and
the RGB correction; the executor itself was moved, not duplicated. No dependencies were added.

Full checks and the embedded cross-check passed. Controller output measured 16.500 ms against
15.895 ms (+3.8%, within Criterion's noise threshold). An unchanged focused run flagged 17.553 ms
with 15 severe high outliers: samples 65-80 contained a concentrated slowdown, reaching 25.43 ms,
while most other ten-sample groups averaged roughly 16-17 ms. A 20-second unchanged run measured
16.625 ms (+4.6%, within the threshold). Each iteration evaluates the same 60 frames, so a changing
show workload does not explain that burst. Scheduling, clocks, and memory/cache state remain
hypotheses; neither the burst's underlying cause nor a smaller persistent code penalty is proven.

## Compiled RGB Packing (2026-09-04)

Elaboration now fuses RGB breakdown, consecutive three-channel reorders, and 8-bit quantization
into one `PackRgb` operation. The operation writes original color bytes in the precomputed order;
it performs no float conversion or quantization. Fusion only removes intermediate values with
one reader, stops at other transformations, and excludes RGBW/discrete color breakdown. Buffer
liveness is recomputed afterward so keeping the original color input alive remains correct.

The starter patch's intermediate payload is now 678 bytes: one 113-color buffer and one 339-byte
slot buffer, down from 3,390 bytes across four buffers. This excludes output frames, element state,
signal workspace, and descriptor/allocation overhead; it is not a whole-show memory total.
Tests assert the two-buffer layout, compare fused and unfused results for all byte values and all
six channel permutations, check composed reorder behavior on starter output, and ensure shared
values, gamma transformations, and RGBW do not get incorrectly removed.

This pass adds 109 implementation lines (94 in host fusion and 15 in runtime packing) and 131 test
lines. The growth is a bounded compiler optimization, not a second evaluator: more work happens
before playback so the common runtime path has fewer passes, no float intermediates, and less
workspace storage.

Validation passed: `cargo fmt`, full `pnpm check`, and `cargo check -p dawn-runtime
--no-default-features --target riscv32imc-unknown-none-elf`. This is cross-compilation evidence,
not physical ESP32 validation.

The full saved-baseline comparison measured controller output at 14.622 ms per 60 frames versus
16.324 ms before this pass (-10.4%; Criterion reports improvement). VM measured 631.01 us,
preparation 566.84 us, representative rendering 4.6547 ms, dense playback 13.907 ms, and cold
playback 14.028 ms; all were classified as no change or within the existing noise threshold.
Cold playback's +3.75% estimate remains recorded rather than being presented as a speedup.
Only controller output exercises this packing optimization; the other timing differences are
not evidence of a causal effect from it. The broader portable-show and patched-preview work
remains incomplete.

## Portable Control and Fixture Behavior Evaluation (2026-09-04)

`dawn-runtime::control` now owns prepared control values, scalar/indexed/fixture control playback,
and fixture behavior evaluation. Elaboration retains target resolution, conflict checking, profile
validation, and lowering. Prepared values use numeric IDs and boxed point/stop slices, not the
authoring `ControlValue`; ordinary indexed controls no longer retain a range curve that their
playback never used. Runtime errors are structured, with existing diagnostic text formatted only
on the host. The old host playback implementation and `output/values.rs` were removed.

Scalar curves and fixture values are sampled once per active clip and then applied to all selected
cells. Timing remains typed u32 microseconds; normalized interpolation still uses f32. The existing
control interpolation boundaries were preserved rather than silently switching to the VM sampler,
whose handling of equal-position points and tiny spans differs. The moved functions require
preallocated fixture-function and explicit-control vectors for allocation-free execution; the
existing output workspace supplies those capacities. This does not claim every VM path is free
of allocations.

The pass adds 76 implementation lines, with no new dependencies or tests: the growth is the frozen
control representation, host lowering, and structured error boundary. There is still one control
evaluator. Full output-session ownership, shared patched preview, and a validated serialized-show
loader remain unfinished. Existing starter output benchmarks do not exercise active scalar or
fixture control clips, so they cannot establish the speedup from sampling a control once per clip.

Full `pnpm check` and the `riscv32imc-unknown-none-elf` no-default-features cross-check passed.
The full benchmark comparison measured controller output at 13.999 ms versus 13.958 ms per 60
frames, with no measurable change. VM measured 611.05 us, preparation 565.64 us, representative
rendering 4.6196 ms, and dense playback 13.786 ms; these were all no change or within the noise
threshold. Cold playback measured 13.537 ms versus 14.922 ms, classified as improvement. Its
pre-change baseline had itself flagged a slowdown on unchanged source; control playback is not
executed by that benchmark, so neither fluctuation is being attributed to this move. The cause
of that baseline variability is still unresolved. No physical ESP32 execution was performed.

## One Portable Show Evaluator (2026-09-04)

`dawn-runtime::show::PreparedShow` now owns the prepared sequence, controls, fixture behaviors,
patch, numeric color spans, output widths, and element-layout descriptors. `workspace()` allocates
the mutable playback storage once; `evaluate(time, buffers, workspace)` executes the complete
sequence-to-patched-byte path into caller-owned `AsMut<[u8]>` buffers. It needs no controller IDs,
network types, authoring project, or host callback. Buffers must follow the prepared output order
and widths. Prepared artifacts and workspace keys must be valid; a validating loader is still
unfinished, so this is not yet an arbitrary-byte artifact ingestion API.

The desktop's `PreparedSequenceOutput` retains preparation, controller metadata, audio/frame
convenience methods, and snapshot copying, but delegates playback to its public `show` field.
There is no second executor or extra copy between runtime output and controller buffers.
Workspace-key allocation stays in host preparation; the new runtime evaluator adds no atomics.
Existing sequence/VM resource ownership and string-valued sequence errors are not eliminated by
this move and remain part of the broader goal.

Prepared element state is now `(ElementNodeId, ElementLayout)`, with u32 cell/function capacities,
rather than retained zero-filled color/scalar/indexed vectors or empty fixture vectors with
reserved capacity. Mutable state is created directly from those descriptors in the workspace.
Color spans likewise use u32 indices/ranges instead of host-sized usize fields. Whole-show retained
RAM remains unmeasured; removing template payloads is not a claim about total workspace memory.

This pass adds 89 implementation lines: 124 in the runtime show module and 35 in element-layout
construction, offset by 85 removed from the host session, plus the public module and host error
conversion. It moves one execution path and adds explicit capacity data; it does not duplicate
renderers. Preview still consumes logical element snapshots and must next be connected to patched
output. The serialized artifact loader and physical ESP32 execution also remain unfinished.

Full `pnpm check`, including existing controller-allocation coverage through the new show path,
and the no-default-features `riscv32imc-unknown-none-elf` cross-check passed. The full benchmark
comparison measured controller output at 14.089 ms versus 13.769 ms per 60 frames (+2.32%, within
the existing noise threshold); this is not a speedup claim. VM measured 618.49 us, preparation
554.44 us, representative rendering 4.5062 ms, dense playback 13.368 ms, and cold playback
13.514 ms. All were classified as no change or within the noise threshold. There were no new
dependencies or tests in this pass.

## Borrowed VM Collection Reads and Array Constraints (2026-09-04)

`Index` and `Len` now encode `RefSlot` operands and borrow their source values. Previously each
read called `Vm::value`, which cloned the collection's Arc/Rc and dropped that temporary after
the read. Selecting a reference-valued element can still clone the selected value; ownership
copies on assignment are not removed. The change adds one compiler line and removes one VM line,
for zero net implementation LOC, and introduces no abstraction or dependency.

Calculated array storage remains unresolved. `MakeArray` collects a new Vec of Values and converts
it to an Arc-backed slice on every execution. `Type::Array` records only the element type, not a
length. The existing `constant_and_calculated_arrays_preserve_nested_values_and_assignment` test
deliberately contains jagged nested arrays, aliases a calculated two-element array, then assigns
a one-element array to the original variable; the alias must preserve the previous contents.
For-loop conditions are also runtime expressions. Reusing one mutable array buffer per expression
without accounting for aliases across loop iterations would change valid show behavior.

The replacement therefore needs capacity and lifetime planning from both bytecode and bound
array parameters; `VmWorkspace::reserve` currently receives only bytecode and a parameter count.
A fixed-size-type assumption, a frame reset alone, or reserving a Vec while still constructing
fresh Arc slices does not solve this. This pass removes unnecessary read-side reference counting,
not the calculated-array allocation, and does not redefine allocation-free playback around the
existing constant-array tests.

Existing DSL tests (including calculated-array aliasing/reassignment), full `pnpm check`, and
the embedded no-default-features cross-check passed. No tests were added; approval was requested
separately for allocation-counted calculated arrays and loop-held aliases.
The full comparison measured VM at 595.34 us versus 597.62 us and controller output at 13.989 ms
versus 13.868 ms per 60 frames, both classified as no measurable change. Preparation measured
546.44 us, representative rendering 4.5951 ms, dense playback 13.562 ms, and cold playback
13.446 ms. All were no change or within the noise threshold; representative rendering's +4.05%
estimate and cold playback's +2.44% are retained here, not described as improvements. This pass
does not establish stable timing or remove calculated-array allocations.

### Calculated-array pool candidate (not implemented)

The next candidate is preallocated indexed storage for immutable calculated arrays, with numeric
handles and non-atomic ownership counts to preserve aliases until their last reader disappears.
Array payload storage must also be preallocated: putting a newly allocated Vec inside a pool
would not solve the problem. Slots must not be recycled while registers, parameter overrides, or
other arrays still refer to them. Merely resetting storage at the start of a frame does not handle
aliases across iterations within a single invocation.

`slab` 0.4.12 is already present transitively in Cargo.lock. Its documented no-std mode and
preallocated indexed-slot reuse make it a candidate for managing free slots, rather than writing
a custom slab allocator. It grows on insertion when full, so preparation must establish capacity
and playback must reject exhaustion before insertion. It does not itself solve array aliasing,
payload storage, or capacity inference. No direct runtime dependency has been added; approval
is required by repository guidance.

Capacity planning depends on representation. Copying arbitrary bound arrays into mutable storage
needs their bound shapes. A handle-based design can instead leave bound/constant arrays in frozen
resource storage and plan calculated-array pools from typed register/override roots, MakeArray
widths, and references from higher-rank arrays. This is a design hypothesis to implement and test,
not a proven memory bound. The existing register-count-only reservation is not that proof.
Focused tests for loop-held aliases, reassignment, nesting, and zero allocations remain pending
approval. No performance, RAM, or no-allocation claim is made for this unimplemented candidate.

## Invocation-local Resource Lifetime (2026-09-04)

VM reference registers previously retained their final owned values after sampling returned.
If a generic reference load copied a prepared curve parameter, that Arc/Rc remained shared into
the next automation update, where `Arc::make_mut` could copy the curve instead of updating the
prepared buffer uniquely. Array and target payloads could likewise outlive the invocation just
because a reusable register still held them.

The private VM now clears reference values and parameter overrides on drop, on both success and
error paths. Register lengths and capacities stay intact. The old next-invocation override clear
was removed, and generator execution explicitly drops the VM before returning its owned emitted
values. This ends local ownership; it does not replace resource IDs or calculated-array storage.

The compiler also stops appending a void fallthrough return to sample effects and operators:
their type checker already requires a color return on every path. Generators retain the void
return. Each sample program loses two instructions, one constant, and one reference slot; programs
without real reference operations no longer need a reference register just for unreachable code.

The pass adds 11 implementation lines (9 VM lifecycle lines and 2 compiler lines), including
explanatory comments, with no new dependency or tests. Focused DSL/generator tests, existing
bound-parameter allocation tests, and the embedded cross-check pass. The allocation test currently
updates automation separately from VM sampling; it does not directly reproduce the copied-curve
interleaving case above. That coverage gap remains explicit pending focused VM test approval.

Full `pnpm check` passed. After discarding an interrupted baseline save, the successful clean
pre-change baseline measured VM at 580.98 us and controller output at 14.069 ms per 60 frames.
The full comparison measured VM at 601.18 us (+3.76% by Criterion) and controller output at
14.075 ms, with no measurable controller change. A 20-second unchanged VM rerun measured
602.38 us (+3.63%). Both VM runs are within the configured 5% threshold, but the repeated increase
is recorded as a possible small performance cost, not dismissed as a speedup or proven noise.
Reference cleanup adds work on invocation exit; its individual contribution versus compiler/code
layout changes or machine variability has not been isolated. The lifetime correction is retained.
The remaining full results were preparation 543.34 us, representative rendering 4.4920 ms,
dense playback 13.734 ms, and cold playback 13.604 ms; all were within threshold or no change.

## Direct Resource Cleanup (2026-09-04)

The reference and override resets now use direct per-slot assignments instead of Clone-based
slice fill. Cleanup still runs on every successful or failed VM invocation, and vector lengths
and capacities remain unchanged. Gradient-parameter lowering was deliberately left unchanged so
this comparison isolates the cleanup source change. No dependency or test was added.

The Windows release object's `Vm::drop` disassembly shrank from 166 to 51 static instructions
(including alignment instructions, excluding relocation records). Its main-body call sites went
from five to two; the generic clone dispatch and String-clone branch disappeared. These were
generated branches, not evidence that cloning Void actually allocated strings. The prologue went
from eight pushed registers and a 0x78-byte stack subtraction to three and 0x30 respectively.
These are x86-64 code-generation measurements, not ESP32 execution or whole-show stack totals.
The change adds eight Rust source lines after formatting, including the explanation for avoiding
slice fill; it adds no runtime abstraction.

Validation passed: `pnpm check`, runtime all-target/all-feature Clippy, and
`cargo check -p dawn-runtime --no-default-features --target riscv32imc-unknown-none-elf`.
The complete saved baseline finished before the source edit; the comparison ran after checks,
without competing builds. VM measured 587.78 -> 566.14 us (Criterion change -4.33%), and
controller output measured 14.008 -> 13.369 ms per 60 frames (-4.56%). Both changes remain
within the configured 5% noise threshold, so the smaller generated cleanup body is confirmed
but a stable execution speedup is not yet established. The other comparisons were preparation
535.64 -> 545.77 us, representative rendering 4.4678 -> 4.3732 ms, dense playback
13.323 -> 12.823 ms, and cold playback 13.609 -> 12.815 ms. Criterion classified all as
within threshold (including cold playback, whose confidence interval reaches the threshold).
These measurements do not establish allocation freedom for calculated arrays or the untested
interleaved copied-curve automation case, nor do they constitute ESP32 hardware validation.

## Direct Gradient Parameter Sampling (2026-09-04)

Gradient parameter indexing now lowers to `GradientParamSample`, mirroring direct curve
parameter sampling. It borrows the parameter instead of emitting `LoadRefParam` followed by
generic `Index`: one fewer instruction and reference register per eligible sampling site,
without the corresponding reference-count increment and cleanup decrement. Local gradient
values still use generic indexing. Parameter overrides remain supported, including raw gradients
obtained from arrays; those retain raw-gradient sampling semantics. No scaling or extra color
rounding is introduced. SparkleComet and ShimmerField use this path in the VM suite.

This adds 32 Rust source lines: five for the instruction, ten for lowering, and seventeen for
execution. It adds no dependency or execution layer, but does increase the typed opcode set.
The existing full `pnpm check` gate and riscv32imc no-default-features compile passed. No tests
were added or modified; existing checksums and allocation coverage passed, but there is not
yet dedicated coverage of the new opcode's raw-gradient reassignment branch.

The complete baseline was saved before editing. VM measured 563.30 -> 523.36 us; Criterion
reported a 7.12% improvement, beyond its 5% noise threshold. A 20-second unchanged repeat
measured 513.69 us (-8.49% by Criterion). This supports a VM-suite improvement, not an ESP32
hardware estimate. Whole-show results did not detect a change: preparation 545.89 -> 547.69 us,
representative rendering 4.5115 -> 4.4837 ms, dense playback 12.900 -> 12.955 ms, cold playback
13.107 -> 13.112 ms, and controller output 13.614 -> 13.548 ms per 60 frames. Calculated-array
allocations, remaining shared resources, and total retained-show RAM remain separate unfinished
work; these benchmark and allocation checks do not prove the complete embedded goal.

## Generator Metadata Outside Playback Programs (2026-09-04)

`BytecodeProgram` no longer owns emitted field names or generated-effect identities. Those
two tables now belong to `CompiledEffect`, and generator execution borrows that definition.
Prepared playback continues to retain only the bytecode, so it no longer carries two unused
generator-table slice headers per program. The source-layout reduction is four pointer-width
words per program (32 bytes on the 64-bit host, 16 on a 32-bit target), not a measurement of
whole-show retained RAM. The temporary VM's optional generator state gains one borrowed
pointer. Compiler ownership and compiled-effect hashing were updated together; no second VM,
new wrapper type, dependency, or test was added. The five edited Rust files total two fewer lines.

This is an ownership-boundary improvement, not complete removal of authoring support from the
runtime crate: `CompiledEffect` and generator execution still live there, and enum/constant
resources still contain authoring data. Full `pnpm check` passed, including existing generator
identity, show checksum, and allocation tests. The no-default-features riscv32imc compile passed.

The baseline completed before editing. VM measured 514.17 -> 534.32 us (Criterion +7.02%,
confidence interval crossing the 5% threshold); a 20-second unchanged repeat measured 526.35 us
(+3.61%, within threshold). The repeated increase is a possible small cost, not a speedup or
proven noise; its attribution to VM state size, code layout, or machine variability is unresolved.
Other results were preparation 543.98 -> 551.43 us, representative rendering 4.5110 -> 4.4469 ms,
dense playback 13.177 -> 12.948 ms, cold playback 13.031 -> 13.060 ms, and controller output
13.658 -> 13.516 ms per 60 frames. All were within threshold or no detected change. The retained
metadata reduction is independent of those timing estimates; ESP32 hardware remains untested.

## Array Storage Dependency Constraints (2026-09-04)

Source/API review narrowed the dependency choice; no allocator was added:

- `offset-allocator` 0.2.0 provides numeric ranges in external storage, but its published
  [source](https://docs.rs/offset-allocator/0.2.0/src/offset_allocator/lib.rs.html) uses `std`.
  Its `reset` also reconstructs its metadata vectors, so reset is not a free per-frame operation.
- `orderly-allocator` supports `no_std`, but its
  [documentation](https://docs.rs/orderly-allocator) explicitly says its BTree bookkeeping
  requests global allocations during operation. It does not meet the allocation-free hot path.
- `rt-alloc` 0.1.0 provides a safe fixed pool over caller-owned storage. Its
  [typed views](https://docs.rs/rt-alloc/0.1.0/rt_alloc/struct.RtPool.html) require `Pod`;
  the current `RuntimeValue`/`Value` with owned Identifier strings and Arc resources cannot be
  stored directly through that API. It also uses 64-byte-aligned payloads, whose overhead would
  need measuring for small arrays. It is a candidate, not a vetted or selected dependency.

These findings favor lowering array elements to scalar words and numeric resource handles
before selecting a payload pool, rather than wrapping the current owned values in another
allocator. Numeric handles alone are not sufficient: alias lifetime tracking, nested values,
payload reclamation, and capacity/fragmentation bounds still need implementation and tests.
The existing constant-array allocation check does not exercise calculated MakeArray storage.
No new performance, allocation-freedom, or hardware claim follows from this dependency review.

## Approved Array and Enum Regression Coverage (2026-09-04)

Three new normally enabled DSL tests now cover loop-held array aliases through 256 iterations,
jagged nested-array reassignment, reuse after VM errors and across program layouts, subset-enum
assignment with different declaration orderings, enums inside calculated arrays, and generated
arrays/enum names remaining valid after register cleanup and another generator invocation.
All pass. The shared array-lifetime fixture is also used by the new allocation regression.

Allocation counters in `bound_params_allocations.rs` are now thread-local so parallel tests
cannot contribute unrelated allocations. The new `calculated_arrays_do_not_allocate_after_warmup`
test is explicitly ignored pending the MakeArray storage replacement, not treated as passing.
Running it directly in both debug and release measured 20 allocation calls for two samples
with two loop iterations and 268 calls with 64 iterations, against the required `[0, 0]`.
Both output-color assertions pass before the allocation assertion fails. Warmup is outside the
measured window; this test does not establish allocation freedom for the first prepared sample.

Reproduce the pending release failure with:

```text
cargo test -p dawn-language --release --test bound_params_allocations calculated_arrays_do_not_allocate_after_warmup -- --ignored --exact
```

`pnpm check` passed with that one explicitly pending test ignored. Remove its ignore marker
when the runtime fix lands; the full embedded goal cannot be complete while this case fails.
This pass changes tests and their fixture only, not production runtime behavior or dependencies.

The regression now also tracks newly allocated live bytes and includes a 9,999-iteration case.
In the Windows release test, two samples at 2, 64, and 9,999 iterations made respectively
20, 268, and 40,008 allocation calls, but peak newly allocated live bytes were only
368, 448, and 448. The net newly allocated bytes returned to zero after sampling in every case.
Thus this fixture demonstrates repeated allocation churn over a small live working set; a pool
sized for cumulative loop allocations would waste memory. This is measured evidence for this
fixture, not a proof of a capacity bound for arbitrary programs.

These byte counts include requested Vec capacity and Arc allocation headers, but exclude the
already-warmed workspace, preexisting frozen resources, and system-allocator bookkeeping.
Reallocation accounting measures logical live requested sizes, not any transient copy internal
to the system allocator. They are not total show RAM or ESP32 layout measurements.
`pnpm check` passes; the zero-allocation assertion remains explicitly ignored and fails when
run directly. No allocator dependency has been added; approval for vendoring remains pending.

## Historical offset-allocator experiment (2026-09-04)

This experiment was removed from the current runtime. Calculated arrays use a
reserved free-slot stack instead; the vendored allocator and its manifest entries
were removed. The remaining text records the prior experiment.

Following explicit dependency approval, `offset-allocator` 0.2.0 was vendored and
declared in the workspace and runtime manifests. Its upstream license, source
provenance/archive checksum, exact local changes, and integration constraints were
recorded beside that source. The allocator algorithm was unchanged: imports used `core`/`alloc`, `nonmax` had
default features disabled, and debug logging plus its dependency are removed.
No application-wide logging flags are changed.

All nine upstream tests pass. Dawn's normally enabled runtime integration test
performs 10,000 variable-sized allocation/free operations while retaining other
ranges, checks non-overlap, coalescing, payload exhaustion and metadata exhaustion,
and observes zero system-heap calls after construction. It uses the dependency
directly; this is not yet a test of pooled VM arrays.

For `Allocator<u32>::with_max_allocs(256, 32)`, Windows x64 measurements show
1,128 inline bytes plus two setup allocations totaling 1,024 requested bytes:
2,152 bytes of allocator metadata before any value storage. The 256 units are
logical offsets, not allocated payload bytes. These are not ESP32 measurements.
The fixed bin table makes a pool unattractive for programs with no calculated
arrays; do not allocate one unconditionally for every program/workspace.

`cargo check -p dawn-runtime --no-default-features --target
riscv32imc-unknown-none-elf` passes. The target-specific dependency feature tree
confirms that `nonmax` has no `std` feature enabled. No physical board was tested.

This step adds 647 vendored implementation lines, 276 upstream test lines and
98 Dawn integration-test lines (1,021 Rust lines total, including blank lines and
comments). No existing VM code is removed or changed in this step. This is an
explicit source/maintenance cost of the approved dependency, not a completed
runtime simplification or a speedup. The previously ignored `MakeArray`
allocation regression remains unresolved; array ownership, reclamation and a
fragmentation-aware prepared capacity bound still need implementation.

`cargo fmt` and the full `pnpm check` pass (the preexisting calculated-array
allocation regression is still explicitly ignored). The mandated baseline-save
completed before edits and the comparison completed after checks. Initial
estimates were VM 505.49 -> 531.06 us and controller output 13.129 -> 13.692 ms
per 60 frames; every comparison was within Criterion's existing 5% noise
threshold. These results are not presented as a speedup.

To investigate the consistent upward shift, the retained old and new benchmark
executables were run back-to-back using Criterion's 10-second measurement
windows, with no concurrent builds and a separate `allocator-old-current`
baseline. The unchanged old VM binary now measured 532.72 us, and the new one
523.40 us (Criterion change -0.94%). Old controller output measured 13.744 ms
versus new 13.854 ms (+0.80%). Both comparisons remain within the noise
threshold. The old binary's own upward shift supports temporal measurement
drift rather than attributing the original 3-5% shift to this unused dependency;
it does not establish the exact cause of the machine's timing variation.

## First classic ESP32 hardware baseline (2026-09-04)

The runtime has now been built and run on the user's ESP32-D0WD-V3 revision
3.1 at 240 MHz. See [the on-board profiling report](esp32_profiling.md) for
exact workloads, frame rates, memory, checksum validation, build sensitivity
and limitations. These hardware results replace speculative desktop-to-ESP32
throughput estimates; they do not indicate that the broader runtime overhaul
or Wi-Fi/physical-output integration is complete.
