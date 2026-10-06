# Performance

The target is representative playback on the ESP32: four outputs of 300 pixels at
100 fps, evaluated on one 240 MHz core, which leaves about 2,000 cycles per
output pixel. Frame deadlines, memory and real shows decide whether a change is
worthwhile; microbenchmarks only help find work. Benchmark commands are in
[testing](testing.md).

Effects stay portable bytecode. Speed comes from what the compiler and
preparation can prove, and from the interpreter design, never from native code
generation.

## Compiler

- **Typed operations.** Arithmetic has one opcode per type and operand shape,
  and numeric comparisons fuse with their branch, so the interpreter dispatches
  once per operation.
- **Dataflow.** Constant and copy propagation across control flow, folding,
  dead-code removal, and liveness-directed destinations. `&&`, `||` and `!`
  lower to branches. A conditional assignment whose skipped side is one pure
  instruction becomes a `Choose`.
- **Staging.** Each program has query, target and pixel stages. Work depending
  only on time and parameters runs once per query; work depending on pixel count
  or target bounds runs when that changes; only pixel-varying work runs per
  pixel. Uniform curve, gradient, mark and array reads are lifted the same way.
  Programs record whether they read progress, spatial coordinates or sections,
  so unused context is never computed.
- **Loops.** Invariant scalar work moves out of loops. A monotonic rejection
  guard can exit a loop early when no later iteration can pass it. The proof
  is structural and never depends on effect names.
- **Division.** Repeated division by an unchanged, provably nonzero denominator
  shares one reciprocal. Smoothstep normalization lowers to arithmetic that this
  proof can simplify. HSV extraction uses a 1 KiB table of byte reciprocals.
  Firmware links a hardware-assisted `__divsf3` that is bit-identical to software
  division (about 75 cycles instead of 200).
- **Float policy.** Real-number algebra may change intermediate rounding and
  signed zero. Identities that would hide a missing value (such as `x * 0`)
  need proof the value is present. Transcendental functions are never folded,
  because host and device implementations differ. Integer arithmetic keeps
  wrapping semantics.

## Preparation

- **Specialization.** Parameters without automation that decide control flow
  are specialized away. Primitive expressions over those parameters in the
  initialization prefix are evaluated, and only their results become bound
  inputs, so differently configured instances still share code. Equal specialized programs are interned.
- **Reciprocals.** A fixed finite nonzero divisor becomes multiplication by a
  prepared reciprocal.
- **Register reuse.** Registers whose lifetimes do not overlap are reused after
  specialization and staging. Doing it earlier would hide single-assignment
  values from staging, so the order matters.
- **Operator input fusion.** When an operator has one current-pixel source, the
  source's body is inlined into the operator and the combined program is
  re-optimized. The source's clock is preserved: seconds quantize to the native
  clock, progress uses the original query, and invalid queries return black.
  Fusion is skipped for shared sources, automated sources, multiple source
  sites, explicit pixel addressing, or when the result would overflow a register
  bank.
- **Compact targets.** Physical positions are stored once for each fixture with
  a spatial consumer. Pixel mappings are run descriptors, or indexed records when
  those are smaller. They stay compact in playback memory and are never expanded
  into tables. Identical mappings share storage. This took the Stanford archive
  from 63,584 to 35,568 bytes and its ESP32 playback memory from 78,308 to
  50,512 bytes.

## Batched interpreter

There is one interpreter, and it runs every program over up to 32 pixels per
instruction dispatch; a single sample is a one-lane run.

- Each primitive register is a row of 32 lanes, and bools are lane masks.
- Initialization runs once and is copied to every lane. Runs span fixtures
  unless the program reads pixel count or target bounds.
- Lanes follow their own control flow. A divergent branch parks one side, the
  lowest parked instruction runs next, and lanes rejoin at joins and loop exits.
  Every instruction is supported; there is no language restriction.
- An instruction whose inputs are all uniform runs once and is copied to the
  other lanes. Preparation marks registers that can never be uniform so their
  instructions skip the check.
- A reference register written by one load holds one value for all lanes; other
  references are per lane, and lanes share one local-array arena.
- Operators evaluate their inputs over the same run when the query time is equal
  in every lane, and fall back to one-pixel runs otherwise. Query-uniform times
  use whole-frame input caches. Nested layers reuse a gathered cell map for each
  run.
- Each operator depth has a preallocated workspace, and graph depths order
  upstream slots first, so evaluation needs no allocation.

Device evaluation on October 3, 2026 (one core, Wi-Fi off, mean over 32 frames
including first use; every frame matched host checksums):

| Workload | Mean ms per frame |
| --- | ---: |
| Stanford section | 5.67 |
| Four layers, three operators, 600 pixels | 72.6 |
| MarkChase, 1,200 pixels | 8.45 |
| MarkPulse, 1,200 pixels | 5.55 |
| ShimmerField, 1,200 pixels | 10.3 |
| Chase/Pulse, 16 layers | 6.82 |
| Trivial pixel-varying effect, 1,200 pixels | 1.86 |
| Selected starter port | 0.20 |

Stanford's cost is dominated by its FreezeFrame operator, which loops over
mostly uniform arithmetic. It uses about 9,100 cycles per output pixel against
the 2,000-cycle target.

## ESP32 memory placement

The ESP32 runs code from flash through a small cache, so hot code lives in
instruction RAM. `firmware/esp32/rwtext_hook.x` places these there, by mangled
symbol prefix:
- the batched interpreter and graph evaluation;
- target lookup;
- the per-lane helpers: curve crossing, mark search, color component, clamp,
  floor, sine and division.

Drop glue stays in flash. On October 3, 2026 the loader used 75,052 bytes of
`.rwtext` beside 51,796 bytes of Wi-Fi code, about 2 KB below the limit. Check
the linker output after growing the interpreter.

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

## Measuring

Measure device timing by uploading a representative show and reading the
loader's `PLAYBACK` lines (see the [firmware README](../firmware/esp32/README.md)).
Keep captures, profiles and Criterion output under `target/`; commit only a short
dated summary here. Rerun rather than cite an old result after code, toolchain,
board or payload changes. Report preparation, evaluation, output packing, DMA
completion and physical LED validation separately.
