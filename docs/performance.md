# Performance

The target is representative playback on the ESP32: four outputs of 300 pixels at
100 fps, evaluated on one 240 MHz core, which leaves about 2,000 cycles per
output pixel. Frame deadlines, memory and real shows decide whether a change is
worthwhile; microbenchmarks only help find work. Benchmark commands are in
[testing](testing.md).

Effects stay portable bytecode. Speed comes from what the compiler and
preparation can prove, and from the interpreter design, never from native code
generation. The pipeline is described in [effect compiler](effect_compiler.md).

## Compiler

- **Dataflow IR.** Definitions compile to a hash-consed graph, so equal
  subexpressions are computed once. Constants fold as nodes are built.
- **Domains and stages.** Each node knows whether it varies per instance, per
  query, per target run or per pixel. Work that depends only on time and
  parameters runs once per query; work that depends on pixel count or target
  bounds runs when that changes; only pixel-varying work is a row, and every
  other value in a strip is computed once. Programs record whether they read
  progress, spatial coordinates or sections, so unused context is never
  computed.
- **Laziness.** Branch arms keep their exclusive work, and a test of `a && b` or
  `a || b` branches on `a` first. A uniform condition runs only its taken arm;
  on a per-pixel condition, arms with at most two cheap operations become a
  branch-free select.
- **Reductions.** Loop-invariant work moves out of reduction bodies. `first`,
  `last`, `any` and `all` stop for each pixel that decides.
- **Typed operations.** Arithmetic has one opcode per type. Single-use curve
  clamps, gradient scales and hue replacements become one instruction: `hsv(h,
  saturation(c), intensity(c))`, or the same with `hue(c) + t`, computes the
  color's components once.
- **Division.** Division by a constant or by a value fixed for the instance
  multiplies by its reciprocal. HSV extraction uses a 1 KiB table of byte
  reciprocals. Firmware links a hardware-assisted `__divsf3` that is
  bit-identical to software division (about 75 cycles instead of 200).
- **Float policy.** Real-number algebra may change intermediate rounding and
  signed zero. Identities that would hide a missing value (such as `x * 0`)
  are not applied. Transcendental functions are never folded, because host and
  device implementations differ. Integer arithmetic keeps wrapping semantics.

## Preparation

- **Specialization.** A choice whose condition is fixed for an instance, but
  whose result varies during playback, keeps only its taken arm; fixed reduction
  bounds become constants. Every other fixed value is evaluated during
  preparation and becomes a parameter slot, so differently configured instances
  share one program. Equal programs are interned.
- **Black signals.** An operator whose input is a disabled or empty layer is
  simplified with that input black; one that becomes black is not prepared.
- **Operator input fusion.** An operator consumed once is substituted into its
  consumer at its only current-pixel sample. Its clock is preserved: seconds
  quantize to the native clock, progress uses the original query, and invalid
  queries return black. Fusion keeps the boundary for shared sources, automated
  sources sampled at another time, several sample sites, explicit pixel
  addressing, or a result beyond the row or nesting limits.
- **Slot reuse.** Liveness over each emitted program shares slots whose
  lifetimes do not overlap. Query and target slots persist across strips and
  are never shared.
- **Compact targets.** Physical positions are stored once for each fixture with
  a spatial consumer. Pixel mappings are run descriptors, or indexed records when
  those are smaller. They stay compact in playback memory and are never expanded
  into tables. Identical mappings share storage.

## Strip interpreter

There is one interpreter, and it runs every program over strips of up to 128
pixels; a single sample is a one-pixel strip.

- A scalar instruction runs once per strip; a row instruction runs one loop over
  the strip's selected pixels. Strips span fixtures unless the program reads
  pixel count or target bounds.
- Selections are ascending pixel ranges. A whole strip is one range; a branch on
  a row condition partitions the ranges, and reductions keep their running,
  participating and contributing pixels the same way.
- Operands are branch-free windows of each bank (see the
  [effect compiler](effect_compiler.md#interpreter)), so one loop body serves
  every kind of operand. Costly per-pixel functions are called rather than
  inlined into every loop.
- Resources are references to parameters, constants or array items; nothing is
  reference-counted while a frame runs.
- Operators evaluate their inputs over the same strip when the sample time is
  uniform, and per pixel otherwise. Query-uniform times use whole-frame input
  caches. Nested layers reuse a gathered cell map for each strip.
- Each operator depth has a preallocated workspace, and graph depths order
  upstream slots first, so evaluation needs no allocation.

Host evaluation on October 6, 2026 (Windows, Criterion and a release harness
over every frame):

| Workload | Result |
| --- | ---: |
| Stanford show, 150 pixels, mean per frame | 7.6 µs |
| Stanford preparation | 5.6 ms |
| Stanford archive | 29,081 bytes |
| `prepare_starter` | 3.06 ms |
| `prepared_effect_suite_4x512_pixels` | 96.9 µs |
| `prepared_600_pixels_4_layers_3_operators` | 432.7 µs |
| `prepared_temporal/standard_echo/1600` | 259.2 µs |
| `prepared_marks/chase` | 15.7 µs |
| `render_playback_dense_60_frames` | 2.89 ms |

Stanford's FreezeFrame samples an empty layer, so preparation removes it; the
rest of the show is its mark effects, gradients and hue operators.

## Apple Silicon

Host evaluation on October 6, 2026 (Apple M5 Max, macOS 26.7, AC power,
Criterion with the benchmark thread at user-interactive QoS):

| Workload | Result |
| --- | ---: |
| Stanford show, 150 pixels, mean per frame | 2.83 µs |
| Stanford show, p99 / slowest frame | 11.9 µs / 194 µs |
| Stanford preparation | 3.4 ms |
| `prepare_starter` | 1.08 ms |
| `prepared_effect_suite_4x512_pixels` | 36.5 µs |
| `prepared_600_pixels_4_layers_3_operators` | 152.9 µs |
| `prepared_temporal/standard_echo/1600` | 107.0 µs |
| `prepared_marks/chase` | 4.85 µs |
| `render_playback_dense_60_frames` | 1.36 ms |

The Stanford rows time every frame of the show, five passes. On efficiency
cores (`taskpolicy -b`) the show averaged 6.9 µs per frame, still far inside a
144 fps frame (6.9 ms).

On macOS the risk is wake-up timing, not evaluation. A `recv_timeout` frame loop
like the live output worker, at 144 fps, woke about 1 ms late (p99 1.7 ms) at
default or user-interactive QoS. Under background throttling, which App Nap
applies to apps that are hidden and silent, it woke 50 to 100 ms late and missed
93% of frames, whatever the thread's QoS. `Info.plist` therefore sets
`NSAppSleepDisabled`; the Preview host runs from the same bundle executable.

## ESP32 memory placement

The ESP32 runs code from flash through a small cache, so the strip interpreter
and graph evaluation live in instruction RAM. The runtime's `iram` feature,
which the firmware enables, gives every function in `dsl/vm/strip.rs` and
`evaluation.rs` `link_section = ".rwtext"`. Closures cannot carry the
attribute, so a per-pixel body that the compiler leaves out of line is a named
`#[inline(never)]` method, and a large instruction's loop runs in its own
method rather than inside `Machine::step`, whose closures the inliner may
outline. `pnpm firmware:build` fails if any code from those two modules is
linked outside instruction RAM, naming the closure or function to move.

On October 8, 2026 (classic ESP32 at 240 MHz, flash DIO at 40 MHz, Stanford
`main` uploaded standalone, its effects active from 64 to 91 seconds), average
evaluation over the active section was 2,925 µs with this placement, 2,913 µs
with only the interpreter and graph evaluation in instruction RAM, and 5,741 µs
with nothing placed, which missed 146 frames. Idle frames were 731, 716 and
1,090 µs. The interpreter's placement halves active evaluation; the helper
placements (sampling helpers, pixel lookups, float division) had no measurable
effect on this show and were removed, freeing 3.2 KB. With attribute
placement only, and no linker script, the same run measured 2,968 µs active and
750 µs idle in 74,164 bytes of `.rwtext`.

Drop glue and once-per-frame work such as automation stay in flash. Flash
addresses in the `.rwtext` literal pools show which flash functions the
instruction-RAM code still calls; large math such as `powf`, `expf`, `logf`
and `tanf` stays behind out-of-line wrappers in flash. On October 6, 2026 the
loader used 77,004 bytes of `.rwtext` beside 51,796 bytes of Wi-Fi code, about
1.2 KB below the limit; on October 8, after removing the helper placements, it
used 73,640 bytes. Check the linker output after growing the interpreter: code size, not
speed, decides what the interpreter may specialize. Operand lookups stay out of
line, one bounds check each rather than one per instruction arm, and only the
cheapest operations have loops per operand kind. Graph evaluation borrows its
frame buffers and operator workspaces apart from the shared sampling workspace
rather than taking and restoring them; the restores inlined about 15 KB of drop
glue into instruction RAM.

Large parts of the evaluation workspace are boxed, because firmware task
futures hold playback by value and those futures live in `.bss`, which takes
DRAM from the core-0 stack.

Controller and release images use opt-level 3, fat LTO, one codegen unit and
abort on panic. `debug = 2` keeps symbols without affecting optimization.

## Hardware results

These summarize device runs whose raw captures were not kept. Each describes the
firmware of its date.

- **September 6, 2026:** continuous I2S output for 13,080 frames at 120 Hz with
  no missed deadline (about 1.8 ms evaluation, 2.5 ms encoding, 6.4 ms
  overlapped DMA per frame) and matching frame checksums. No LEDs were attached.
- **September 30, 2026:** on a QuinLED Dig-Quad, the following passed: upload
  replacement, scheduled start, cancellation, elapsed position, pause, seek,
  stop, and the desktop transport test. Best UDP round trip 2.3 ms.
- **October 1, 2026:** the full Stanford show on 150 pixels passed repeated
  replacement, damaged-upload rejection, frame checksums, and scheduled playback
  with audio, and played visibly on LED1. Clock skew and audio alignment were
  not measured.
- **October 6, 2026:** the strip interpreter played the whole Stanford show
  (150 pixels, 41,520 frames at 120 Hz, `i2s-output` image) with matching frame
  checksums and no evaluation allocations: evaluation averaged 908 µs, the
  busiest 5% of one-second windows averaged up to 2.8 ms, and the slowest frame
  took 5.6 ms. One frame missed its deadline while playback started. At least
  39.2 KB of heap stayed free. With its language helpers still in flash the
  same interpreter averaged 960 µs; the previous 32-lane interpreter, measured
  the same day on the same show, averaged 1,250 µs (3.4 ms busy windows, 6.2 ms
  slowest frame).

## Measuring

Measure device timing by uploading a representative show and reading the
loader's `PLAYBACK` lines (see the [firmware README](../firmware/esp32/README.md)).
Keep captures, profiles and Criterion output under `target/`; commit only a short
dated summary here. Rerun rather than cite an old result after code, toolchain,
board or payload changes. Report preparation, evaluation, output packing, DMA
completion and physical LED validation separately.
