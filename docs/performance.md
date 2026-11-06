# Performance

The target is representative playback on the classic ESP32 at 100 fps,
evaluated on one 240 MHz core. The firmware drives four outputs of up to 200
pixels each (`OUTPUT_PIXELS` in `firmware/esp32/src/bin/loader.rs`), which
leaves about 3,000 cycles per output pixel at full load. Frame deadlines, memory
and real shows decide whether a change is worthwhile; microbenchmarks only help
find work.

Effects stay portable bytecode. Speed comes from what the compiler and
preparation can prove, and from the interpreter design, never from native code
generation. The pipeline and the interpreter are described in
[effect compiler](effect_compiler.md).

## Measuring

Measure device timing by uploading a representative show, such as
`examples/stanford_room`, and timing its frames through `POST /frame` (see the
[firmware README](../firmware/esp32/README.md#uploading-and-measuring)). Report
preparation, evaluation, output packing, DMA completion and physical LED
validation separately.

Host benchmarks use Criterion; the commands and workloads are in
[testing](testing.md#benchmarks):

```bash
pnpm bench:effect-vm:quick     # smoke pass
pnpm bench:effect-vm:save      # baseline before optimization work
pnpm bench:effect-vm:compare   # compare against that baseline
pnpm bench:effect-vm           # full set
```

The `*_large_show*` cases in `render_bench` run Ding Dong from
`examples/rydell_house` and need its uncommitted `audio/dingdong.mp3`.

- Pin timing threads. On hybrid CPUs an unpinned thread drifts to efficiency
  cores and runs 3–4x slower; the benchmarks pin themselves
  (`pin_benchmark_thread`), and ad hoc harnesses must do the same.
- Compare A and B in the same session, and rerun rather than cite an old result
  after code, toolchain, board or payload changes.
- The example shows change constantly and are not frozen workloads, so numbers
  measured on them are approximate and only comparable within one session.
- Keep captures, profiles and Criterion output under `target/`; commit only a
  short dated summary here.

## Recent results

| Date | Workload and conditions | Result |
| --- | --- | --- |
| 2026-10-18 | Stanford `main` (150 pixels) on a QuinLED Dig-Quad, every frame from 64 s to 91 s through `POST /frame`, format 71 | 4.67 ms mean evaluation, matching checksums; `.rwtext` 74.4 KB |
| 2026-10-18 | Rydell House `all_we_are` (25,314 pixels), 55–62 s, Windows x64 release on an i9-13980HX, pinned to CPU 2 | 6.88 ms per frame |
| 2026-10-27 | `prepare_large_show_warm` / `prepare_large_show_cold`, same host, pinned | 15.7 ms warm |
| 2026-10-11 | `rydell_house` project check (30 documents, 36 MB), same host, pinned | 1.7 s cold, 1.5–2.2 s after an edit |
| 2026-10-06 | Whole Stanford show (150 pixels, 41,520 frames at 120 Hz) with continuous I2S output | 908 µs mean evaluation, slowest frame 5.6 ms, one missed deadline at start, no evaluation allocations |

## ESP32 memory placement

The ESP32 runs code from flash through a small cache, so the strip interpreter
and graph evaluation live in instruction RAM; the rule and its build check are
in [development](development.md#firmware-runtime-constraints). Placing the
interpreter halves active evaluation. Instruction RAM is nearly full, so code
size, not speed, decides what the interpreter may specialize: check `.rwtext` in
the linker output after growing it. Large math such as `powf`, `expf`, `logf`
and `tanf` stays behind out-of-line wrappers in flash.

Controller and release images use opt-level 3, fat LTO, one codegen unit and
abort on panic. `debug = 2` keeps symbols without affecting optimization.

## macOS

On macOS the risk is wake-up timing, not evaluation. Under background
throttling, which App Nap applies to apps that are hidden and silent, a
144 fps frame loop woke 50 to 100 ms late and missed most frames whatever the
thread's QoS. `Info.plist` therefore sets `NSAppSleepDisabled`; the Preview host
runs from the same bundle executable.
