# Performance and hardware evidence

Donder treats representative playback cost, frame deadlines, and retained memory
as acceptance evidence. Isolated microbenchmarks help locate work but do not
justify complexity by themselves. The repeatable Criterion workflow is in
[Regression tracking](regression_tracking.md).

## Compiler and interpreter optimizations

Arithmetic bytecode uses individual typed operations (`FloatAdd`, `IntAdd`,
and their peers). Immediate float operations encode operand order in the opcode.
The interpreter therefore selects arithmetic once, without an arithmetic-family
handler dispatching through another operation table. Numeric conditional jumps
combine comparison and branching; their polarity preserves negated comparisons,
including comparisons involving a missing value.

Compilation performs scalar/color constant and copy propagation over control flow,
intersecting facts at joins and iterating across loop back-edges. It folds known
arithmetic and conditions, removes unreachable code and dead pure calculations,
and uses liveness to let producers write directly into a following assignment's
destination. Conditional `&&`, `||`, and `!` lower directly to short-circuit
branches. Signal sampling and operations that can report errors remain ordered
and observable, even when their result is unused. Typed parameter resource
samples are total: missing curves produce NaN and missing gradients produce black.
They can move across control flow when their inputs are invariant. These passes retain no analysis
state in playback and introduce no runtime cache.

Host preparation specializes fixed inputs that determine control flow, including
fixed numeric mode expressions. Automated inputs remain dynamic. Ordinary gain,
timing and color settings remain parameters so invocations can share bytecode.
Equivalent specialized programs are interned during preparation; the temporary
dependency analysis is discarded. Known clip duration and target population may
participate when they determine control flow.

Preparation also evaluates primitive expressions in the immutable initialization
prefix when all inputs are fixed bindings or constants. Only the final values
needed by the remaining program become bound inputs; intermediate
calculations and unused fixed-input loads disappear. The resulting instructions
remain independent of binding values, so differently configured invocations can
share code. Unused fixed-input slots of the same type hold prepared results before
new slots are appended; automation addresses stay unchanged. Automated inputs,
time/geometry reads and resource sampling remain
dynamic. This uses the compiler's existing constant evaluator and ordinary
parameter instructions, with no new playback analysis or sampled-frame cache.
Fixed finite nonzero divisors with finite reciprocals become multiplication in
the pixel body. Their reciprocals use the same prepared-binding storage; repeated
loads of one parameter share one reciprocal, independent of its numeric value.
Zero, nonfinite, overflowing-reciprocal and changing divisors retain division.
The pass preserves the existing initialization boundary and compacts register/
resource storage; it does not repeat whole-program control-flow optimization.

Initialization has query, target and pixel stages. Time/parameter expressions run
once per query; expressions depending on pixel count or target bounds rerun when
that domain changes. Pixel-varying expressions remain in the body. The compiler
also treats repeated pixels as a loop when proving invariant division, and
section-position sampling uses a shared width reciprocal. Programs record whether
they read progress, spatial coordinates or sections so unused context work can be
omitted.

Uniform typed curve/gradient samples participate in query, target and loop
staging. When a fused sample-and-scale or sample-and-clamp instruction combines
uniform sampling with varying arithmetic, compilation separates those operations
and lifts the sample. Gradient gain still clamps to 0–1 before color scaling;
ordinary channel saturation alone would change the result. Fully varying samples
keep their fused instruction. No resource references or sampled frames are cached.

Structured-loop analysis moves total, invariant scalar calculations before their
loop. It preserves values observed after a zero-iteration loop and respects
conditional definitions. A monotonic rejection guard can jump directly out of
its loop when subsequent iterations cannot pass it: the induction counter must
not wrap, the rejected tail must have no observable work, and earlier iterations
must not contain sampling or other potentially failing work before the guard.
The proof follows values and control flow; it has no effect or operator names.
It covers additive counters and multiplication by a nonnegative constant, for
either sign of the initial counter. Known iteration counts tighten the literal
loop cap. The proof includes the final update and rejects wrapping or
sign-alternating progressions; live values and source errors remain observable.

Repeated division by an unchanged denominator can share one reciprocal. Range
analysis must prove that nonmissing denominators stay away from zero far enough
to avoid reciprocal overflow. The reciprocal is an ordinary scalar register,
computed after the denominator's definition and, when eligible, in the existing
uniform prefix. Changing, unbounded, and potentially tiny divisors keep division.
Both passes run only during compilation, use existing opcodes, and add no runtime
analysis, allocation, cache, or alternate execution path.

Smoothstep edge normalization lowers to ordinary subtraction and division before
the clamped cubic instruction. Constant folding and the same invariant-divisor
proof can therefore remove repeated edge arithmetic and software division.
Varying and degenerate widths retain ordinary division semantics.

Float optimization permits real-number algebra and changes to intermediate
rounding and signed zero, including multiplication by a precomputed reciprocal
for a finite nonzero constant divisor. This is a compiler policy, not a global
Rust fast-math switch. Missing resource samples still use NaN: annihilating
identities such as multiplication by zero require a value proven present, so
`value_or` can still observe missing samples. Integer arithmetic retains wrapping
and total remainder behavior. Transcendental constant folding is excluded because
host and device use different implementations.

HSV component extraction uses byte-domain reciprocals stored in 1 KiB of
read-only constants. Hue and saturation denominators are integers from 1 to 255;
intensity and hue normalization also multiply by constant reciprocals. This
removes software division without initialization, heap storage or frame state.
Intermediate floating-point rounding may change under the same real-number
algebra policy.

The ESP32 linker selector includes generic arguments in the mangled VM method
name, placing the shared interpreter in instruction RAM. The effect-entry
wrapper and its literals reside there too. Prepared bytecode
uses current wire format 45; previously exported sequences must be regenerated.

ESP32 profiling and controller images use Cargo's release profile: optimization
level 3, fat LTO, one codegen unit and abort-on-panic. `debug = 2` retains symbols
for attribution; it does not disable release optimization.

Instruction fetching advances a local slice through the bytecode. Sequential
execution no longer reloads, increments and stores a numeric instruction index
or scales that index by the instruction size on every dispatch. Taken branches
rebase the slice from their absolute target. Register storage stays in the
existing workspace. Bounds checks remain in place, including for trusted
archive bytecode, whose operands are not revalidated during decoding. Fetching
requires no additional cache or per-frame allocation.

Arithmetic reads input registers directly before writing the result, preserving
in-place operands without copying input rows. Register addresses and operation
selection are shared across active lanes. Register resizing remains separate
from the hot constructor.

Preparation reuses nonoverlapping primitive register lifetimes after binding,
specialization and uniform staging. Prefix registers and live-in values stay
pinned; control-flow liveness includes branch joins and loop backedges. Reference
registers retain their existing ownership. Allocating before specialization can
hide single-assignment values and move uniform work into the pixel body, so the
allocation order is part of the optimization contract.

Three generic arithmetic pairs combine multiplication with addition, constant
multiplication with addition, or multiplication with normalized smoothstep.
Fusion runs only in the pixel body after staging. It requires a single use of the
intermediate result and cannot cross a branch entry. Multiply/add retains two
arithmetic operations rather than requiring fused hardware rounding. These
instructions share dispatch, register addressing and lane traversal.

### Bounded block execution

Admitted operators whose scalar control/data is identical across pixels execute
color operations over blocks of up to 32 pixels. Temporal loops therefore
initialize and query upstream programs once per block instead of once per pixel.
Source effects share their query initialization across the block, and uniform
sources are evaluated once for each block/query. Tail blocks, seek changes and
explicit local/global addressing retain their original sampling domains.

Programs with primitive temporary registers can also execute numeric operations
over 32 lanes. Lanes at the same instruction execute together; divergent
branches retain one pending instruction pointer per lane and rejoin when control
flow converges. Bounded loops may have different iteration counts, and a returned
lane stays complete while its peers continue. Reference-valued temporaries retain
ordinary scalar execution. Direct parameter-curve and gradient sampling can run
in numeric lanes without creating reference-valued temporaries.

Admission proves eligibility and retains a small execution-mode tag; it performs
no frame analysis. Register capacity is bounded by 32 times the program's
primitive slots, independent of frame size or temporal sample count. Query
initialization executes in lane zero, then broadcasts once at the stage boundary.
Target initialization runs once when a block shares pixel count and target bounds,
and those registers survive adjacent same-query blocks. Blocks crossing different
domains initialize each lane separately. Section and pixel reads retain each
lane's original domain. Same-query blocks reuse initialization even when their
active width changes. Eligible operators do not allocate full-frame temporal caches.

Numeric operator lanes require current-pixel source addressing at query-uniform
times. Operators with exactly one such source instruction can also
traverse upstream in blocks. The source must address the current pixel, and its
time must be defined in the validated query prefix. On the first source request,
evaluation fills up to 32 temporary colors; each lane then runs the operator's
ordinary scalar code against its source color. Branches, arrays, numeric color
operations and bounded loops keep their scalar semantics and per-operation color
quantization. The temporary expires after the block, even for repeated timestamps;
it is not a retained frame cache. This scheduling applies inside block traversal;
whole-frame entry points retain their existing contiguous source pass. Replacing
that pass with small blocks can repeat upstream initialization unnecessarily.
Nested traversal instead benefits by keeping a scalar operator from forcing every
upstream layer to repeat its uniform initialization for each pixel. Adjacent blocks
also preserve the effect workspace's initialized registers when the effect and
sample time are unchanged; switching either invalidates that reuse.

Samples and operators share the same native interpreter specialization and
28-byte Xtensa instruction representation. Sample admission still rejects signal
queries; effects do not require a caller-provided signal sampler. Lane count is
selected per invocation. Constant RGB/HSV/color arithmetic folds through the same
sampling functions used at runtime. Float-seconds-to-clock conversion uses the
f32 significand and integer scaling, preserving nearest-tick rounding and range
errors without software double-precision arithmetic.

Whole-frame source scheduling remains contiguous; nested traversal can carry
numeric blocks through hue/saturation-dependent operators.

Host preparation inlines private operator inputs when the caller has one
current-pixel source instruction. The source body appears once, including when
that instruction is inside a loop. Register, resource, input and parameter
addresses are remapped, then the ordinary compiler passes simplify the combined
program. Unreachable loops lose their private state slots as well as their code.
Graph assembly retains only the resulting nodes, bindings and workspace depths.

Fusion preserves the queried source clock explicitly: seconds are quantized to
the native clock, progress uses the original query and sequence duration, and
invalid or out-of-sequence queries skip the body and return black. Source errors,
branches, early returns and per-operation color quantization remain observable.
Both programs must share sequence duration and pixel/section domains. Shared
graph sources, source automation, multiple source sites, explicit pixel
addressing and reliance on prior workspace contents retain their boundaries.
Fusion also refuses to turn a block-capable caller into a scalar-only program.
It adds no retained sample cache or runtime graph-analysis state.

The [current measurements](../firmware/esp32/results/accepted/2026-10-03-wide-arithmetic.json)
cover late register allocation, 32-lane execution, direct register reads and
generic arithmetic fusion. The preceding
[program-fusion report](../firmware/esp32/results/accepted/2026-10-03-program-fusion.json)
provides the eight-lane baseline. Historical stage measurements remain in the
accepted evidence directory; the architecture above describes current behavior.
All 640 current-math host reference frames preserve their checksums, timestamps
and active-effect counts. The device verifies 576 current frames across 18 workloads
plus 160 unfused arithmetic control frames in the same image, with zero evaluation
allocations. Those controls isolate arithmetic fusion; the complete eight-to-32
lane comparison uses separate native images.
Earlier reciprocal math changed 84 of 57,600
channels in the added mixed workload by at most one byte level; subsequent passes
introduce no further output changes.

| Workload | Previous / current mean ms | Current p95 ms | Current max ms |
| --- | ---: | ---: | ---: |
| Four layers, three operators, 600 pixels | 106.703 / 89.079 | 89.246 | 89.724 |
| ScanSweep, 150 pixels | 1.786 / 1.800 | 1.802 | 2.163 |
| ImpactBurst, 150 pixels | 1.697 / 1.721 | 1.727 | 2.092 |
| SparkleComet, 150 pixels | 2.718 / 2.822 | 2.824 | 3.187 |
| Shimmer, 1,200 pixels | 14.662 / 15.140 | 15.240 | 15.592 |
| Stanford section | 13.823 / 13.428 | 13.945 | 14.597 |
| Starter port | 0.233 / 0.193 | 0.191 | 0.541 |

These 32-frame windows include first use. Measurements use a classic ESP32 at
240 MHz, one core, Wi-Fi off and untouched GPIO. They exclude network, storage,
DMA and physical output and do not establish full-show worst cases or a general
5x gain. The controller image is built and inspected separately.

Mixed frame time falls 16.5% (1.20x throughput). Disabling only arithmetic fusion
in the same image gives 91.698 ms, so those three instruction pairs save 2.619 ms
here. Stanford changes from 13.468 to 13.428 ms in that isolated comparison; the
unaffected Shimmer and ScanSweep controls are effectively identical.

Several isolated effects improve little or regress slightly. Shimmer at 1,200
pixels rises by 0.478 ms; the faster, oversized candidate measured 14.761 ms,
the compact iterator 14.990 ms, and the shared-helper candidate 15.114 ms before
final formatting. The smaller lane/control implementation therefore carries a
measured speed/size tradeoff. All 32 frames of that 1,200-pixel case remain below 16.667 ms;
the much larger mixed-workload gain is retained rather than adding more code to
recover these small isolated costs.

The mixed workload has four different simultaneous starter effects followed by
hue shift, a two-repeat 75 ms echo, and dimming. The echo samples the complete
upstream mix three times: twelve effect samples per output pixel. Relative to
the staged/block baseline, its preceding eight-lane mean fell from 291.705 to
106.703 ms. Wider execution improves this further, but does not remove the
7,200 upstream effect samples per frame.
That remains far above the 16.667 ms budget. The workload is a diagnostic
reference, not a universal claim about four-layer shows.

The width sweep used identical prepared programs. Eight lanes is the original
iteration reference; 16/32/64 use the same dense/sparse implementation. Mixed means were 105.127 ms
at eight lanes, 107.132 ms at 16, 97.993 ms at 32 and 97.724 ms at 64. The final
implementation uses 32: 64 cost another 4,864 retained bytes for negligible
improvement. These intermediate timings precede direct register access and
arithmetic fusion; percentages from different stages should not be added.

Packed scalar storage for uniform numeric registers saved another 564 workspace
bytes in its controlled comparison, but slowed mixed playback from 109.283 to
115.570 ms. Per-operand scalar/lane addressing and the larger interpreter
outweighed the avoided broadcasts. It is not retained. Early register reuse also
regressed performance by hiding uniform expressions from specialization; moving
allocation after staging corrected that cause.

Dense active lanes advance by shifting a mask one bit. Only gaps call a shared
software bit-search helper. Outlining this uncommon path avoids copying Xtensa's
software `trailing_zeros` implementation into every instruction handler. Color
writes likewise share their scalar/broadcast helper. Both remain in instruction
memory. Branch scheduling scans only the invocation's lane count.

| Playback allocation, bytes | Mixed previous / current | Stanford previous / current |
| --- | ---: | ---: |
| Archive | 12,401 / 12,076 | 38,965 / 38,640 |
| Decoded sequence | 15,224 / 14,860 | 49,112 / 48,748 |
| Workspace and outputs | 10,172 / 12,780 | 6,756 / 9,772 |
| Total retained | 25,396 / 27,640 | 55,868 / 58,520 |

At eight lanes, late register reuse alone saves 1,040 mixed workspace bytes.
Wider batches spend that saving and additional space on transient lane storage.
No new retained sampled-frame cache is added, and no cache-hit improvement has
been measured. Stack reservations are separate from the allocation table.

Remaining mixed-workload self-symbol shares are 63.5% shared VM, 5.7% block-layer
traversal, 3.9% software float division and 8.7% gradient sampling. Stanford is
58.0% VM and 23.2% division. These are weighted PC-sample
shares across two sampling periods. The shared native symbol cannot distinguish
effect and operator callers. Numeric blocks amortize dispatch, but register
access and color/math conversion still execute for every active pixel. Removing
eligible graph/program boundaries helps Stanford but barely changes the mixed
workload. Further large gains require reducing the remaining per-pixel
calculations and register/dispatch work, or avoiding repeated upstream evaluation
where semantics permit it. The profiles do not establish a single dominant opcode.
In particular, the VM share includes arithmetic, color conversion and register
access; it is not a measurement of dispatch overhead alone. At 89.079 ms for
7,200 effect samples, the mixed workload consumes about 2,969 cycles per sample
including operator/composition costs. The 60 Hz budget is about 556 cycles per
sample before physical-output and system costs. Wider scalar batches share
bookkeeping, but do not perform multiple pixels' arithmetic simultaneously.

Both the ordinary I2S and Dig-Quad controller release variants build and link with
identical instruction/static-RAM budgets. They retain the configured
network-core stack region of 20,396 bytes. `.rwtext` occupies 78,164 bytes and
`.rwtext.wifi` 51,796, leaving only 88 bytes in the instruction region. The shared
interpreter occupies 38,352 bytes, versus 38,996 previously. Initial wider-loop
implementations exceeded this region; sharing sparse iteration and color writes
allows optimization level 3 to remain in use. Runtime size optimization fit but
regressed mixed playback to 114.683 ms and is not retained.

The output controller reserves a 24 KiB render-core stack instead of 6 KiB.
Its ordinary heap region decreases from 92 to 74 KiB, preserving the network-core
stack region; with the separate 64 KiB region, total heap is 138 KiB. This is an
explicit stack/heap tradeoff, not an additional allocation during evaluation.
Including the mixed show's additional 2,244 retained bytes, available heap
headroom decreases by about 20.2 KiB assuming other allocations are unchanged.
That is a meaningful memory cost for the 1.20x throughput gain.
The standalone profiler has a 160 KiB heap. Networked upload/load peaks, including
Wi-Fi allocations and an in-flight archive, were not measured in this pass; the
smaller controller heap can reduce the size of shows it can load.
The interpreter itself reserves 272 stack bytes. Recursive 32-lane evaluation
also retains lane contexts in callers, so individual function sizes cannot be
used as a maximum stack bound. Profiling measures stack use separately from frame
timing. These workload measurements do not prove a bound for arbitrary graph
depth, and the networked controller's complete output path is not timed here.

### Compact prepared targets

Prepared sequences keep physical positions once per fixture that has a spatial
consumer. Targets store original sampling bounds as runs, independently of
physical positions. Spatial and section metadata are retained only for the
targets whose consuming programs need them. Filtering an output does not change
the original target's indices, fractions, bounds or section population.

Regular physical/logical pixel mappings use run descriptors. Irregular mappings
use indexed records when those occupy fewer archived bytes. Both are executable
representations: loading does not expand runs into pixel tables. Sequential
traversal advances through runs; random sampling and membership queries search
the compact mapping. Original fraction bits reside in shared immutable tables,
so reconstruction introduces neither floating-point division nor new rounding.
Identical mappings share their storage even when target scopes or section
semantics differ. Section membership similarly chooses runs or indexed records.
All sharing is established during preparation; there is no new playback cache.

The same compact structures remain in decoded playback memory. A sampled pixel
is reconstructed by value; its existing address is reused when reading shared
positions and scope bounds. Nonspatial programs borrow a static unused context
instead of constructing six placeholder floats per sample. Neither path allocates.

Operator VMs remain in their preallocated slots during recursive evaluation.
Prepared graph depths put upstream slots before their consumers, so evaluation
splits the mutable workspace slice at the current operator and lends only the
preceding slots to its inputs. This follows the existing automation-state borrow
pattern. Individual VMs are no longer moved out and replaced with empty register
banks for each sample; only the containing Vec header moves once per frame.
This changes neither sample reuse nor allocated workspace capacity and adds no
cache, unsafe access or panic-based slot extraction.

The [reviewed compact-target evidence](../firmware/esp32/results/accepted/2026-10-02-compact-targets.json)
compares this representation with the instruction-fetch baseline below. All 608
host frame CRCs, active-effect counts, timestamps and dynamic instruction
signatures match. The archive layout changes, but the executed bytecode does not.

| Workload | Archive before / after bytes | ESP32 decoded before / after bytes | ESP32 playback before / after bytes |
| --- | ---: | ---: | ---: |
| Stanford section | 63,584 / 35,568 | 72,560 / 44,764 | 78,308 / 50,512 |
| ShimmerField, 1,200 pixels | 50,714 / 7,562 | 51,200 / 8,084 | 62,244 / 19,128 |
| FreezeFrame over black | 7,807 / 2,455 | 7,980 / 2,664 | 9,284 / 3,968 |

Playback totals include decoded data, workspace and output buffers. Workspace
and output-buffer sizes are unchanged; all measured device evaluations allocate
zero times. Dense starter's archive falls from 471,610 to 68,354 bytes. On the
64-bit host its retained playback allocations fall from 672,265 to 151,577 bytes;
this workload was not run on the ESP32 and does not establish controller capacity.

Host allocation diagnostics also measure decode/preparation peaks, excluding
the input archive, allocator metadata and stack. Dense starter's peak falls
from 672,265 to 151,577 bytes; Stanford from 126,425 to 87,537; and Shimmer 1,200
from 92,354 to 20,522. Adding the archive length estimates simultaneous input
plus preparation storage at 219,931, 123,105 and 28,084 bytes respectively after
the change. These are host measurements and derived sums, not measured firmware
upload high-water marks. No laptop timing comparison was made.

Playback speed is mixed; smaller storage did not produce a broad speedup:

| Workload | Before / after mean ms | Change in time |
| --- | ---: | ---: |
| Stanford section | 21.643 / 22.107 | +2.1% |
| FreezeFrame over black | 11.364 / 11.083 | -2.5% |
| ShimmerField, 150 pixels | 3.065 / 3.076 | +0.4% |
| ShimmerField, 1,200 pixels | 23.900 / 24.358 | +1.9% |
| Chase/Pulse, 16 layers | 23.165 / 23.957 | +3.4% |
| Starter port | 0.701 / 0.752 | +7.3% |

These windows include first use. Warm controls are 21.292 / 21.861 ms for
Stanford, 11.267 / 11.039 for FreezeFrame, 2.983 / 3.048 for Shimmer 150, and
0.605 / 0.669 for the short starter wrapper. Simple black and sparse scheduling
add 34–37 us. Stanford still misses the 60 Hz evaluation budget in all 32
sampled frames; FreezeFrame fits 60 Hz and still misses 120 Hz. Network, storage,
DMA and physical output remain outside these measurements.

Compact traversal trades direct record loads for address arithmetic and lookup.
Native inspection also found duplicate pixel reconstruction, unused-context
stores and whole-VM move/default/drop work. The current evaluator removes those
costs rather than restoring expanded tables. An out-of-line context-wrapper
experiment increased Stanford to 26.135 ms; inlining that wrapper, bypassing
single-run searches and borrowing workspace slices recovered most of the loss.
Native code layout also changes between these variants, so the remaining deltas
cannot be assigned entirely to one operation or to cache behavior. The measured
heap savings are retained despite the modest playback tradeoff.

In the compact-target baseline, the main hotspots were the interpreters and division: Stanford self-symbol
samples are 27.5% operator VM, 26.6% effect VM and 19.1% software float division.
Compact target lookup accounts for 2.4%. FreezeFrame spends 40.6% in the operator
VM and 22.2% in source-pixel sampling; Shimmer spends 73.9% in the effect VM.
These are statistical self samples, not separately timed functions. Bytecode
instruction counts have not fallen, and no cache-hit improvement was measured.

Borrowing VMs in place also reduces native stack frames: the graph evaluator
falls from 656 to 384 bytes and the recursive source sampler from 608 to 384.
Both interpreter frames remain 160 bytes. These individual frames do not measure
maximum nested stack usage. At that stage the controller release build used 3,720
additional instruction-RAM bytes, leaving 17,360 bytes free with its then-configured
156 KiB heap unchanged. It was built and linked; the board measurements used the
standalone profiling image. All 544 device reference frames pass, and incomplete
serial captures are excluded from the reviewed evidence.

### Previous ESP32 instruction-fetch results

The [reviewed instruction-fetch evidence](../firmware/esp32/results/accepted/2026-10-02-vm-access.json)
compares the advancing cursor with the loop-optimized baseline below. All 19
prepared archives, their manifest, 608 host frame CRCs and dynamic instruction
counts are identical. Each device capture verifies 544 frames across 17 cases.

| Workload | Before / after mean ms | Time reduction | After p95 ms |
| --- | ---: | ---: | ---: |
| Stanford section | 22.774 / 21.643 | 5.0% | 22.085 |
| FreezeFrame over black | 11.914 / 11.364 | 4.6% | 11.350 |
| ShimmerField, 150 pixels | 3.340 / 3.065 | 8.2% | 3.089 |
| ShimmerField, 1,200 pixels | 26.087 / 23.900 | 8.4% | 24.005 |
| Chase/Pulse, 16 layers | 25.520 / 23.165 | 9.2% | 23.184 |

These are the same 32-frame evaluation windows, including first use. Unsampled
warm controls also improve: Stanford 22.434 to 21.292 ms, FreezeFrame 11.825 to
11.267 ms and Shimmer 3.257 to 2.983 ms. The short starter wrapper remains
essentially unchanged at 0.604 to 0.605 ms warm; its first-use mean rises by
7 us. Sparse scheduling rises by 1 us. These small changes do not justify
additional runtime complexity. Stanford still exceeds the 60 Hz evaluation
budget on every sampled frame. Network, storage, DMA and physical output are
outside this measurement.

Register-access experiments explain why more local pointers did not help.
Borrowing all nine banks as slices eagerly copied pointers and lengths into the
interpreter's stack frame, including unused banks. Its operator/effect frames
grew to 240/224 bytes, versus 160/160 for the cursor alone. A smaller split
register/array/loop reference reduced this to 176/160 and recovered much of the
loss, but still did not beat the cursor-only workloads. In the generated code,
the workspace base already occupies a CPU register: the nested Rust field path
does not imply another pointer chase. The current implementation keeps that
smaller ownership structure. The reviewed evidence retains both experiments.

The remaining Stanford self-symbol shares are 27.7% operator VM, 27.3% effect
VM and 19.7% software float division, corresponding to approximately 5.91,
5.82 and 4.20 ms when multiplied by its warm mean. These are sampled estimates,
not separately timed functions. Division's larger share does not demonstrate a
new division regression. Shimmer remains 75.9% effect VM; FreezeFrame spends
39.1% in the operator VM and 28.7% in source-pixel sampling. Further work should
target the remaining instruction work and repeated sample setup, rather than
assuming source-level register indirection is expensive.

Retained heap is unchanged for every case and evaluation allocation counts are
zero. The native interpreter stack frames increase from 128/112 to 160/160 bytes
for operator/effect execution; these individual frames are not a measurement of
maximum nested call-stack use. The cursor's native code is also larger because
taken branches now include slice-rebasing checks. That controller build
used 1,488 additional bytes of instruction RAM, leaving 21,080 bytes free, with
its then-configured 156 KiB heap unchanged. It was built and linked; these timing
results come from the standalone profiling image. Raw captures, matching images
and assembly remain under `target/vm-opt-20261002/access-*` and
`target/vm-access-20261002/`.

### Compiler loop and division results

The [reviewed loop-optimization evidence](../firmware/esp32/results/accepted/2026-10-02-loop-optimization.json)
compares the loop-optimized compiler with the preceding compiler/interpreter result in the
next section. Workloads, parameters and timestamps are identical. This round
measures ESP32 performance; the host supplies preparation, instruction counts
and output-equivalence checks, without another laptop timing comparison.

| Workload | Before / after mean ms | Time reduction | After p95 ms |
| --- | ---: | ---: | ---: |
| Stanford section | 32.880 / 22.774 | 30.7% | 23.285 |
| FreezeFrame over black | 21.832 / 11.914 | 45.4% | 11.897 |
| Chase/Pulse, one layer | 3.414 / 3.115 | 8.8% | 3.123 |
| Chase/Pulse, 16 layers | 27.271 / 25.520 | 6.4% | 25.541 |

These are 32-frame evaluation windows including first use, without Wi-Fi,
storage, DMA or physical LED output. FreezeFrame fits the 60 Hz evaluation budget
on every sampled frame, while Stanford still exceeds it on every sampled frame.
FreezeFrame also still exceeds the 120 Hz budget. A sampled section does not
establish whole-show worst-case deadlines.

Stanford executes 29,462 instructions per frame, down from 60,811; FreezeFrame
executes 15,013, down from 46,451. Both retain exactly the same source-sample
counts (1,200 and 750 respectively). FreezeFrame's ordinary division count falls
from 751 to one, plus one reciprocal calculation. Chase/Pulse benefits from
sharing divisors across expressions, demonstrating that the passes also help
code outside temporal operators and loops.

All 608 host frame CRCs match the baseline, and all 544 device frames verify
against the host manifest. Evaluation allocates nothing. Retained memory rises
by 88 bytes for Stanford and 32 bytes for FreezeFrame, including bytecode and
ordinary scalar registers. That compiler-only round produced the exact same
controller binary hash as its preceding baseline; native code and its memory
footprint were unchanged. Only prepared bytecode changed.

After the loop passes, Stanford self-symbol samples were 29.6% operator VM, 29.1% effect VM and
18.0% software float division. Applying these shares to the 22.43 ms warm mean
estimates 6.65, 6.53 and 4.04 ms respectively. The preceding estimates were 15.65,
6.32 and 5.00 ms: the main reduction is in operator execution. These are sampled
estimates, not isolated function timings. Division includes native work inside
other instructions and source evaluation, so its cost does not fall in direct
proportion to the count of division bytecodes. Source-pixel sampling now accounts
for 23.6% of FreezeFrame's time; its estimated absolute cost remains about 2.8 ms.

Shimmer's bytecode and instruction count are unchanged, and its warm mean is
essentially stable (3.263 / 3.257 ms). The starter port's one-microsecond increase
in the first-use window is accompanied by lower warm controls (0.615 / 0.604 ms).
Neither change justifies additional runtime complexity. The accepted evidence
contains all 17 workloads, memory deltas and hotspot sample counts.

### Initial dispatch and dataflow results

The October 2 optimization comparison uses the same workload definitions,
parameters and timestamps as the profile below. Both interpreters are in IRAM
in every ESP32 variant in this comparison. The original flash-resident VM
measurements below are historical evidence, not the optimization baseline.

The [reviewed optimization evidence](../firmware/esp32/results/accepted/2026-10-02-vm-optimization.json)
records each intermediate, confidence intervals, checksums, memory and hotspots.
Laptop times are Criterion slope estimates; device times are the means of the
32-frame windows. The final device column includes the entry-wrapper placement.

| Workload | Laptop original / optimized us | ESP32 original / optimized ms | ESP32 time reduction |
| --- | ---: | ---: | ---: |
| Stanford section | 333.18 / 230.23 | 49.275 / 32.880 | 33.3% |
| FreezeFrame over black | 258.55 / 169.48 | 31.237 / 21.832 | 30.1% |
| ShimmerField, 150 pixels | 28.36 / 24.41 | 5.247 / 3.404 | 35.1% |
| ShimmerField, 600 pixels | 110.94 / 97.30 | 20.561 / 13.173 | 35.9% |
| SparkleComet, 150 pixels | 45.34 / 40.11 | 8.015 / 5.562 | 30.6% |
| Chase/Pulse, 16 layers | 224.22 / 174.22 | 31.095 / 27.271 | 12.3% |

At this earlier stage, Stanford and FreezeFrame exceeded a 60 Hz evaluation
budget on every sampled frame. ShimmerField at 600 pixels and four 150-pixel
layers fit the evaluation budget in that window. These are not full controller
deadline measurements.

The flat-opcode intermediate separates dispatch changes from compiler changes.
The compiler reduces Stanford from 88,997 to 60,811 executed instructions per
frame and FreezeFrame-over-black from 71,142 to 46,451. Stanford's `Move` count
falls from 18,031 to 450 per frame; FreezeFrame's falls from 15,389 to zero.
These are dynamic host counts over the same 32 timestamps, including nested
sampling. They do not add counters to the timed executables.

The earlier instruction profile showed affine loop expressions executed 4,800
times per frame, motivating the general loop passes above. Recorded pairs and
triples include control-flow edges: frequency alone does not prove that
instructions are adjacent, their operands form a fusible expression, or their
cost is removable.

All 608 sampled host frame CRCs match across the original, flat-opcode and fully
optimized builds. Each accepted device capture verifies 544 frames against its
host manifest and reports zero evaluation allocations. Stanford's device
retained heap falls from 81,308 to 78,220 bytes. The compiler's worklist and
liveness sets exist only while compiling.

The initial laptop suite showed a 4.6% ImpactBurst slowdown. Interleaved reruns
with unchanged binaries measured 24.0-24.1 us optimized versus 26.4-26.6 us
original, so that regression did not reproduce. The evidence retains both runs.
The very short black/scheduling cases vary by tens of nanoseconds on the host
and tens of microseconds on the device; these did not justify additional VM
complexity while the representative cases improved.

The flat-opcode intermediate made ShimmerField slower on ESP32 despite lowering
the interpreter's sampled self time. Samples instead accumulated at the
flash-resident effect-entry wrapper immediately before its indirect VM call.
Moving only that wrapper and its literals into IRAM reduced warm ShimmerField
from 5.408 to 3.634 ms, with identical bytecode and checksums. This establishes
placement sensitivity; interrupted PCs do not identify the exact cache miss or
bus-stall mechanism. With all compiler optimizations, wrapper placement has
little additional effect (3.406 to 3.404 ms in the 32-frame mean). The current
hook keeps the wrapper beside the VMs and replaces its obsolete method selector.

Before the loop passes, ESP32 self-symbol samples put Stanford's operator VM at 48.2%, effect VM
at 19.4%, and software float division at 15.4%. Multiplying those shares by the
32.50 ms unsampled warm mean gives approximately 15.65, 6.32 and 5.00 ms;
these are estimates, not separately timed functions. Original IRAM estimates
were 22.77, 8.92 and 4.78 ms. Division therefore occupies a larger share after
removing other work, without a demonstrated reduction in its absolute cost.
FreezeFrame remains dominated by the operator VM (66.4%) and source-pixel
sampling (13.5%). ShimmerField is 77.1% effect VM and 11.0% float division.

Updated laptop CPU sampling was blocked by Windows administrator consent for
Samply's ETW helper. Laptop hotspot tables below describe the original binary;
the optimization's laptop evidence is the fresh Criterion timing and instruction
counts, not a new CPU hotspot attribution.

That stage's full `dig-quad` Wi-Fi/storage/I2S firmware linked with `.rwtext` at
55,684 bytes and `.rwtext.wifi` at 51,796 bytes, leaving 22,568 bytes of IRAM
after vectors. Its then-configured 156 KiB heap was unchanged. Compared with the
original IRAM-only build, native placement consumes 5,452 additional bytes;
Stanford playback retains 3,088 fewer heap bytes. The full loader was built but
not flashed or timed; measured performance comes from the standalone image.

The missing-scalar regression found during development was semantic: blindly
folding `sample * 0` hid the NaN that `value_or` uses to recognize a missing
sample. The optimizer now requires a proven-present scalar for that identity.
Existing missing-value tests and the focused control-flow, loop, copy and
short-circuit tests pass. No runtime fallback was introduced.

Validation completed with `cargo fmt`, `pnpm check`, the full `dig-quad` release
build, and all 59 cases in `pnpm bench:effect-vm:compare`. The prepared four-effect
512-pixel suite decreased from 438 to 378 us. Dense controller output stayed
close to baseline (8.30 to 8.14 ms for 60 frames); dense logical rendering was
unchanged within noise (8.00 to 8.04 ms). Standard Echo at 800 pixels decreased
from 1.64 to 1.10 ms. The saved workspace baseline overlapped some build work,
so the isolated laptop comparisons above are the primary speedup evidence.
No case was classified as a regression at the configured 5% noise threshold;
checksum and active-effect assertions remain unchanged.

## October 2 VM profile: laptop and ESP32

The [reviewed measurements](../firmware/esp32/results/accepted/2026-10-02-vm-profile.json)
cover 19 laptop workloads and 17 workloads on the attached ESP32. The evidence
supports reducing interpreter work before adding playback caches. Temporal
operators execute many simple instructions; the cost of signal traversal and
operator setup matters when the bytecode itself is short. These baseline
measurements precede the compiler/interpreter changes described above; no new
playback cache was introduced in either stage.

### Measurement boundaries

The laptop is an Intel Core i9-13980HX running Windows 11 Pro, Balanced power
plan. Criterion used one thread pinned to logical CPU 2, 1 second warmup,
3 seconds measurement, and 50 samples. The table reports its slope estimate;
95% confidence intervals are retained in the evidence. Native code used Rust
1.98.1, normal release optimization and debug symbols. Timing excludes decoding
and instruction-count instrumentation.

The device is a classic ESP32 revision 3.1 at 240 MHz with 4 MB flash. A standalone
profiling image uses one core, no Wi-Fi, and no LED GPIO output. It uses the
shared runtime with atomic reference counting, opt-level 3, fat LTO and one
codegen unit. Each workload evaluates 32 specified timestamps, including its
first evaluation. Timings cover prepared evaluation and output-buffer encoding;
the checksum calculation runs outside the measured interval. Device p95 is the
31st sorted observation out of 32, so it is a small-window statistic, not a
long-run latency guarantee. A slow first frame can make mean exceed p95.

Every one of the 544 device frames matched the host's combined color/output
CRC32. All 17 cases recorded zero evaluation allocations and released their
retained heap after playback. These are evaluation-budget checks, **not measured
DMA deadlines or complete controller playback**. Network, upload, storage,
I2S encoding/DMA, physical LEDs and audio synchronization are outside this run.

Instruction counts were collected separately on the host from the same archives
and timestamps. The observer records executed opcodes and within-invocation
pairs/triples, including loops and nested signal calls. It resets history at
each VM invocation. Its allocation-heavy maps exist only in an ignored host
diagnostic snapshot; its timings are never used as playback results. Counts
are host observations, not direct ESP32 instruction counters. Existing runtime
prefix/uniform-sample reuse remains enabled in all builds.

### Playback cost and scaling

| Workload | Pixels | VM instructions/frame | Laptop us/frame | ESP32 mean / p95 ms | ESP32 retained KiB |
| --- | ---: | ---: | ---: | ---: | ---: |
| `ScanSweep_150` | 150 | 6,159 | 26.65 | 7.427 / 7.418 | 10.5 |
| `ImpactBurst_150` | 150 | 7,208 | 28.52 | 7.371 / 7.445 | 10.7 |
| `SparkleComet_150` | 150 | 12,013 | 47.55 | 10.009 / 10.006 | 11.8 |
| `ShimmerField_150` | 150 | 7,521 | 28.62 | 5.853 / 5.863 | 10.8 |
| `ShimmerField_600` | 600 | 30,021 | 116.39 | 22.630 / 22.678 | 32.4 |
| `ShimmerField_1200` | 1,200 | 60,021 | 238.22 | 44.927 / 45.026 | 61.1 |
| `ShimmerField_150_layers4` | 150 | 30,084 | 117.69 | 22.542 / 22.591 | 13.8 |
| `ShimmerField_150_layers16` | 150 | 120,336 | 469.04 | 89.302 / 89.504 | 25.8 |
| `black_150` | 150 | 5 | 0.65 | 0.103 / 0.093 | 7.9 |
| `freeze_black_150` | 150 | 71,142 | 280.55 | 40.706 / 40.715 | 9.3 |
| `scheduled_16` | 150 | 5 | 0.44 | 0.071 / 0.065 | 9.8 |
| `scheduled_128` | 150 | 5 | 0.46 | 0.078 / 0.072 | 27.3 |
| `scheduled_1024` | 150 | 5 | 0.78 | host only | - |
| `chase_pulse_150_layers1` | 150 | 8,263 | 28.44 | 4.034 / 4.025 | 7.1 |
| `chase_pulse_150_layers4` | 150 | 16,270 | 57.65 | 8.029 / 8.025 | 10.4 |
| `chase_pulse_150_layers16` | 150 | 65,072 | 232.44 | 31.693 / 31.704 | 20.6 |
| `starter_port1` | 113 | 235 | 4.90 | 0.918 / 0.906 | 36.1 |
| `starter_dense` | 3,390 | 6,890 | 137.04 | host only | - |
| `stanford` | 150 | 88,997 | 336.11 | 79.058 / 82.338 | 79.4 |

The first four effects use the existing runtime benchmark definitions. Pixel
and layer variants hold ShimmerField's effect parameters fixed. The layers
intentionally repeat the same work, so matching output does not mean matching
execution cost. Chase/Pulse uses the existing alternating-effect fixture;
increasing from one to four layers changes its mix, not just its layer count.

The synthetic samples span approximately 3.000-3.258 seconds. FreezeFrame uses
the project-local operator with interval 0.1 seconds and fade 0.5 seconds over
a constant-black input. Its fixed 32-iteration loop still runs for every pixel,
even though most snapshots are rejected. The black comparison isolates the
added operator work for this input; it is not permission to replace FreezeFrame
with black for arbitrary inputs.

Scheduled cases contain 16/128/1,024 non-overlapping 100 ms clips, with one active
at a time and samples crossing clip boundaries. Increasing stored clip count
barely changes absolute frame cost in these windows, although retained memory
grows. These results do not describe thousands of simultaneously active clips.

`starter_dense` evaluates the maintained starter project's `layer_test` sequence
at frames 8,420-8,451, approximately 58.472-58.688 seconds. It has 15-30 active
effects; many source effects are uniform, leaving signal traversal as a useful
stress case. `starter_port1` prepares just the first controller port and has one
active effect. `stanford` evaluates the local Stanford project at
70.000-70.258 seconds, with six active effects on 150 pixels. It is a sampled
section, not a whole-show average. Its archive and standalone image differ from
the October 1 controller capture below, so 79 ms versus the older 41.5-48.9 ms
is **not a controlled regression comparison**.

At a 60 Hz evaluation budget (16.667 ms), every sampled frame exceeded budget
for ShimmerField at 600/1,200 pixels, its four/sixteen-layer variants, FreezeFrame,
sixteen-layer Chase/Pulse and Stanford. SparkleComet exceeded 120 Hz's 8.333 ms
budget on all 32 frames; four-layer Chase/Pulse exceeded it on the first frame.
Passing evaluation alone leaves output and transport costs still to budget.

### Where CPU samples land

Host profiles use Samply for 12 seconds per case and matching executable/PDB
symbols. Approximately 12,000 on-CPU samples per case fall inside Criterion's
profiling routine. ESP32 profiles use interrupted PCs at 997 and 1,999 us,
bracketed by unsampled windows. Each window lasts at least two seconds and
completes whole 32-frame cycles. The accepted capture contains every declared
sample and completed all cases. Failed serial captures remain unaccepted under
`target/`; buffered collection completed successfully.

The following are **self samples by machine-code symbol**, including any helpers
inlined into that symbol. They are not inclusive call-tree percentages or pure
dispatch cost. Device ranges are the two sampling rates, not confidence bounds.

| Workload | Laptop hotspots | ESP32 hotspots |
| --- | --- | --- |
| ShimmerField, 150 | VM 57.4%; sine 19.5%; gradient sampling 9.3%; copy helper 3.1% | VM 71.3-73.4%; copy helper 10.7-13.2%; float division 6.8-7.2%; gradient 3.3-4.8% |
| FreezeFrame over black | Operator VM 64.1%; source-pixel sampling 15.5%; copy helper 7.6%; effect VM 3.4% | Combined VMs 80.2-80.6%; source-pixel sampling 6.3-7.9%; copy helper 5.3-5.4% |
| Stanford section | Combined VMs 66.9%; previous-mark scan 8.3%; copy helper 6.8% | Combined VMs 69.1-73.0%; array access 6.0-6.8%; float division 5.8-6.1%; copy helper 4.0-4.2% |
| Dense starter / selected port | Full show: operator setup 23.7%; operator VM 23.0%; graph traversal 16.1%; signal sampler 9.5% | Selected port: combined VMs 31.6-34.3%; graph traversal 13.7-13.8%; signal sampler 11.3-12.0%; operator setup 5.2% |

The last row uses different output scopes and is not an identical-workload
percentage comparison. The selected port also spends 16-19% of samples outside
sized ELF symbols. ROM address inspection attributes most of these to double
arithmetic/conversion routines. With no device stack samples this does not prove
which caller owns each ROM sample; time conversion is a candidate to investigate.

Samples attributed to the VM's instruction-fetch, IP-increment and match lines
sum to 30.7% on the laptop for FreezeFrame and 27.3% for Stanford. On ESP32 those
lines account for about 20% and 17-18%, respectively. This supports investigating
dispatch work, but optimized source-line attribution is approximate: these
percentages must not be treated as a promised removable cost. The rest of VM
time includes instruction bodies, register access, branches and inlined helpers.

Device sampling raised warm per-frame time by 0.34-0.40% at 997 us and 0.17-0.24%
at 1,999 us. The profiler uses a fixed 16 KiB PC buffer outside playback heap.
Unsampled continuous-loop means were 5.628 ms ShimmerField, 40.477 ms FreezeFrame,
0.726 ms selected starter port and 78.349 ms Stanford. These differ from the
32-frame table because the table includes first use and interleaves checksum
work between frames. Host timing runs had no sampling enabled; host sampling
overhead was not separately quantified.

### Frequent instructions and useful fusion candidates

These are dynamic shares, not counts of instructions merely present in an
archive. Stanford executes about 89,000 instructions/frame: Move 20.3%,
JumpIfFalse 12.6%, float subtraction 12.2%, float multiplication 7.1%, IntToFloat
5.9%, comparison against a constant 5.6%, float comparison 5.4%, integer addition
5.4%, and LoopEnd 5.4%. FreezeFrame's distribution is similar. Of Stanford's
576,995 moves across 32 frames, 356,198 are boolean, 158,400 integer, 33,597 float
and 28,800 color; none is a reference-counted resource move in this window.

| Executed pattern | Workload | Occurrences/frame | Interpretation |
| --- | --- | ---: | --- |
| Float comparison -> Move -> JumpIfFalse | Stanford | 10,100 | Includes constant and register comparisons; first examine redundant boolean copies and direct conditional branches |
| Integer add -> Move -> LoopEnd | Stanford | 4,800 | Examine loop-variable copies before introducing a special loop opcode |
| IntToFloat -> subtract -> multiply | Stanford, FreezeFrame | 4,800 | Computes `(capture_index - float(snapshot)) * interval`; strong straight-line fusion candidate |
| Multiply -> subtract | Stanford | 4,950 | Has a producer/consumer relationship; intermediate snapshot time is also used later, so it cannot simply be discarded |
| Divide -> subtract from 1 | Stanford, FreezeFrame | 750 | Fade amount; smaller than loop-control patterns |
| ColorScale -> ColorMax | Stanford, FreezeFrame | 750 | Another compact arithmetic candidate |
| Multiply -> add | ShimmerField, 150 pixels | 300 | Common phase calculation |
| Sin -> add 1 -> multiply 0.5 | ShimmerField, 150 pixels | 300 | Repeated normalization; sine itself is also costly |
| SignalSample -> ReturnColor | Dense starter | 3,390 | A possible return-signal operation; traversal/setup still dominates much of this workload |

Straight-line candidates above were checked for adjacency and producer/consumer
register use; control-flow patterns were counted separately. Counts overlap and
cannot be added into a speedup estimate. These are candidates, not completed
liveness proofs. A compiler transform must preserve branch targets, live
intermediates, short-circuit behavior and observable sampling order. The current
real-arithmetic policy above permits reciprocal multiplication and intermediate
rounding changes while retaining the missing-sample contract.

### A concrete ESP32 placement problem

Before the correction, the [IRAM hook](../firmware/esp32/rwtext_hook.x) claimed to
place `Vm::run` in instruction RAM. The baseline ELF placed both versions in flash:
operator VM at `0x400eba80`, 15,655 bytes, and effect VM at `0x400f26f8`, 15,353
bytes, both in `.text`. The old selector required the contiguous mangled substring
`2Vm3run`; the compiler's v0 symbols put generic arguments between `2Vm` and
`3run`, so the selector misses them. Signal graph/layer traversal and float
division do match their selectors and reside in `.rwtext`.

Disassembly also shows the existing dispatch already uses an indirect jump table,
with instruction bounds checking, an IP update, and a 28-byte instruction stride.
A rewrite justified only as replacing a long chain of opcode comparisons would
therefore target the wrong mechanism on this build.

The follow-up experiment below measures the placement correction. The two VM
bodies alone require 31,008 bytes (30.3 KiB) of instruction RAM, before literals.
Placement must be verified from the final ELF rather than inferred from linker
comments.

### Measured IRAM follow-up and remaining hotspots

The [IRAM comparison](../firmware/esp32/results/accepted/2026-10-02-vm-iram.json)
changes only the diagnostic snapshot's native-interpreter selector to
`*(.literal.*2Vm*3run* .text.*2Vm*3run*)`. Both VM functions are verified in
`.rwtext`; bytecode, effect parameters, timestamps, checksums and optimization
settings are unchanged. This relocates native interpreter code, not the decoded
bytecode or a new playback cache. The board was flashed with the IRAM profiling
image, and all 544 frame checksums matched with zero evaluation allocations.

| Workload | Flash mean ms | IRAM mean ms | Time reduction |
| --- | ---: | ---: | ---: |
| Stanford section | 79.058 | 49.275 | 37.7% |
| FreezeFrame over black | 40.706 | 31.237 | 23.3% |
| ScanSweep, 150 pixels | 7.427 | 3.782 | 49.1% |
| ImpactBurst, 150 pixels | 7.371 | 4.020 | 45.5% |
| SparkleComet, 150 pixels | 10.009 | 8.015 | 19.9% |
| ShimmerField, 150 pixels | 5.853 | 5.247 | 10.4% |

The following hotspot comparison pools the two sampling periods. Approximate
milliseconds are **derived** from sample share times the bracketing unsampled
warm-frame mean, not measured with a per-function stopwatch. Warm Stanford
means were 78.349 ms in flash and 48.881 ms in IRAM; these differ slightly from
the 32-frame first-use table above.

| Stanford component | Flash share / approximate ms | IRAM share / approximate ms | Laptop share |
| --- | ---: | ---: | ---: |
| Operator VM | 43.2% / 33.9 | 46.6% / 22.8 | 52.0% |
| Effect VM | 27.2% / 21.3 | 18.3% / 8.9 | 14.9% |
| Separate float-division helper | 5.9% / 4.6 | 9.8% / 4.8 | No comparable separate symbol |
| Copy helper | 4.1% / 3.2 | 7.0% / 3.4 | 6.8% |
| Gradient sampling | 2.7% / 2.1 | 4.8% / 2.3 | 2.0% |

The interpreter remains the largest hotspot, with its combined sampled cost
falling from approximately 55.2 to 31.7 ms. Division and copies did not become
substantially slower in this workload: their percentage increased mostly because
the interpreter got faster. The laptop also concentrates work in the interpreter
and copies, but its separate previous-mark scan accounts for 8.3%. Inlining and
code generation differ, so an absent device helper symbol does not mean that
operation costs nothing; device mark work is partly inside the VM.

Within the IRAM Stanford VM, about 24% of total CPU samples map to instruction
fetch/IP update/dispatch, approximately 11.8 ms per warm frame. Float arithmetic
handler regions account for another approximately 8%. Many leaf locations are
register-vector indexing and access. These are optimized debug/source regions,
not a precise division into dispatch, bounds-check instructions, arithmetic,
memory stalls and callees. External division and copy helpers are separate from
the VM self percentages. A full per-opcode cycle attribution would require
additional instrumentation; dynamic instruction counts alone do not provide it.

In this earlier capture, the two VM symbols were specializations of the same
`Vm::run` implementation in `crates/donder-runtime/src/dsl/vm.rs`. Effect programs
used an uninhabited signal capability and operators used a
`&mut dyn SignalSampler`; current playback shares one native specialization.
An operator's `SignalSample` instruction converts its
time/pixel arguments and asks the graph sampler for a color. That can read a
prepared frame or enter an upstream effect/operator at the requested time.
Preallocated workspaces preserve the suspended caller's registers. These are
nested ordinary calls, not separate threads or a new allocated interpreter per
sample. Operator self time excludes time executing the upstream effect VM.

Splitting the IRAM Stanford source attribution by the containing VM symbol gives:

| Region inside VM machine code | Operator VM approximate ms | Effect VM approximate ms |
| --- | ---: | ---: |
| Fetch / IP / dispatch | 8.26 | 3.58 |
| FloatArithmetic handler, including inlined operand access | 3.06 | 1.00 |
| Float comparisons and JumpIfFalse combined | 2.86 | 0.37 |
| SignalSample's own instruction body, excluding out-of-line callees | 1.37 | Not supported |
| Mark handler region | Not sampled | 0.95 |
| Whole VM symbol, including remaining/unattributed regions | 22.77 | 8.92 |

Fetch/IP/dispatch is about 36% of operator VM self samples and 40% of effect VM
self samples. These source-region estimates have the same optimized-debug-info
limits as the combined attribution; shared machine-code tails can inherit a
source location that does not uniquely identify the executed opcode. Separate
division (4.78 ms), copy (3.43 ms) and gradient (2.34 ms) helpers are outside both
VM symbols. The interrupted-PC profiler does not retain caller stacks, so it
cannot reliably divide those shared helper costs between effect and operator
callers. Nor does this capture isolate the signal callback's indirect-call cost.

Other workloads expose different remaining costs:

- **FreezeFrame over black:** after IRAM, combined VMs are 72.8%, source-pixel
  sampling 10.1%, and copies 7.8%. Fetch/IP/dispatch regions account for about
  26.8% of total samples. The fixed loop still executes about 71,000 instructions
  per frame; placement did not remove any of those instructions.
- **ShimmerField:** laptop VM 57.4%, sine 19.5%, gradient 9.3%, copies 3.1%; IRAM
  ESP32 VM 57.9%, gradient 21.7%, copies 10.2%, division 5.3%. Sine is inlined
  into the ESP32 VM, so comparing its separate symbol shares would be misleading.
  The gradient helper's derived cost rose from approximately 0.21 to 1.11 ms
  after VM relocation; both sampling periods show the change. This is larger
  than a denominator effect. Code/cache-layout interaction is a hypothesis,
  not an established cause, and needs an isolated placement experiment. The
  workload still improves overall by roughly 9% in uninterrupted warm loops.
- **Short signal wrapper:** IRAM selected-starter-port samples concentrate in
  the operator VM (26.3%), graph traversal (20.7%), operator setup (8.6%), signal
  sampler (8.5%) and unresolved ROM code (15.2%). The laptop full-starter profile
  also highlights setup/traversal, but it covers all outputs rather than this
  one port and is not a quantitative like-for-like comparison.

These profiles motivated the compiler copy/branch cleanup and reduced
per-instruction dispatch measured above, with division and gradient helpers
especially relevant on ESP32. The laptop alone would overemphasize its standalone
sine/mark-scan symbols and would miss flash-layout effects. There is no measured basis here
for assuming one optimization gives the same percentage improvement on both.

The initial `dig-quad` Wi-Fi/storage/I2S placement-only build linked both VMs in
IRAM, retaining its configured heap and 28,020 bytes of unused IRAM. That build was
not flashed or exercised; all timing remains from the standalone profiler.
The retained October 1 loader ELF already had its older, non-generic VM in IRAM.
The generic VM's selector mismatch therefore does not establish a deliberate
decision to require flash execution. The current product includes the correction.
Raw captures, matching ELFs, inline-symbol analysis and
comparison scripts are in `target/vm-profile-20261002/iram/`.

### Further investigation

After compiler cleanup and direct arithmetic/branch dispatch, select any further
superinstructions from the remaining dynamic sequences. Short operator setup,
signal/time conversion, and previous-mark scans remain separate possible targets.
Use the IRAM build as the embedded baseline, inspect generated assembly, and
retain bytecode admission checks. These profiles do not establish a need for
unchecked indexing or additional playback caches.

For acceptance, retain the pixel/layer sweep, sparse scheduling cases, short
signal wrapper, FreezeFrame-over-black and real-project section as separate
workloads. A longer next-stage stress sequence should combine abrupt overlap
changes, dense marks, nested temporal operators, parameter automation and seeks,
then run through the real controller scheduler/I2S path. Track evaluation p95/max,
actual missed deadlines and peak/retained RAM. The present short windows do not
establish long-run worst-case behavior or maximum stack use.

### Memory and reproducibility

The device's Stanford retained heap is 81,308 bytes: 75,508 decoded sequence plus
5,800 workspace/output storage. FreezeFrame-over-black retains only 9,496 bytes,
despite its high CPU cost. Full starter retains 674,255 requested heap bytes on
the 64-bit host; the 1,024-clip case retains 330,836. Those two were intentionally
excluded from the 160 KiB device harness. Host requested allocation sizes and
ESP allocator accounting are different measures; neither includes full process
memory, stack, allocator metadata, profiler storage or all firmware globals.
Archives are embedded in flash for device profiling and excluded from retained
playback heap. No evaluation allocation was observed in any host case either.

All detailed artifacts remain under `target/vm-profile-20261002/`: identical
archives and timestamp/checksum manifest in `cases/`, opcode/pair/triple TSVs in
`counts/`, Criterion results in `target/criterion/`, four native profiles and
symbols in `host/`, and the exact `device.elf`, verified serial capture and
symbolized hotspot summaries. The source snapshot and diagnostic harnesses are
there too. The reviewed JSON retains workload/image hashes, timing intervals,
memory, sample counts, leading hotspots and instruction patterns; raw traces
are not promoted. The 201 recorded original source files remained unchanged.

The snapshot's `profile-host` package uses Criterion for timing; it is not a new
product benchmark CLI. To rerun existing archives locally:

```text
cargo bench --manifest-path target/vm-profile-20261002/source/Cargo.toml -p profile-host --bench playback_profile -- --noplot
cargo run --release --manifest-path target/vm-profile-20261002/source/Cargo.toml -p profile-host --features vm-profile --bin count-profile
py -3.11 target/vm-profile-20261002/profile_host.py
```

`analyze_counts.py`, `analyze_host.py` and `analyze_device.py` reduce the captures.
`capture_device.py` requires pyserial, resets COM3, verifies all declared samples
and checksums, and refuses to overwrite an existing raw capture. Its matching
ELF must already be flashed. Archive prior captures inside `target/` before a
new collection. Ordinary ongoing benchmarking continues to use the repository's
Criterion commands below. The connected board was left with this profiling
image, replacing its previous firmware as authorized.

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
