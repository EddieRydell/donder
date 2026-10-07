# Donder

Donder is a desktop studio for pixel-LED light shows. You lay out your lights,
sequence effects against music on a timeline, wire them through a composition
graph, and play the result live over the network or upload it to an ESP32
controller that runs the show on its own.

Every effect is a program you can read and change, every show is a folder of text
files, and the controller runs the same engine as the preview.

## Effects are programs

Effects and operators are written in Donder's pure, expression-oriented effect
language. They compile into one dataflow graph per sequence, which preparation
simplifies and lowers to portable bytecode. The bundled libraries (Chase, Wipe,
Spin, mark-triggered pulses, and [eighteen Vixen effects](docs/vixen_effects.md))
are ordinary source files in your project. Open one, change it, or write your
own:

```text
effect Pulse {
  param gradient: gradient;
  param pulse_shape: curve in 0.0..1.0;

  sample {
    gradient_color_scaled(gradient, progress, pulse_shape[progress])
  }
}
```

An effect samples one color per pixel per frame. It can read the pixel's index,
its position in layout space, the target's bounds, curves, gradients, beat marks
and time. Parameters declared with `param` appear as editable, automatable
controls in the editor, bounded by their declared ranges.

## Composition is a graph

Effects are drawn onto layers, and layers feed a node graph that ends at the
output. Operators are programs too, and they can sample their inputs at any time
and any pixel:

```text
operator TimeWarp {
  input source;
  param offset_seconds: float in -1.0..1.0 = 0.0;

  sample { source.at(time + offset_seconds) }
}
```

That one call is enough to build delay, echo, freeze frames, time warps, mirrors
(`source.at(time, target.count - 1 - pixel.index)`) and spatial blends.
The standard library includes Max, Add, Multiply, Dim, Invert, Colorize,
HueShift, IntensityModulate, Delay and Echo. Disconnected branches stay in the
graph while you work on them.

## Automation clips drive anything

An automation clip is a curve placed on the timeline. It is not tied to one
parameter: each clip binds to any number of parameters on any effects or graph
operators, mapped onto each parameter's declared range. One clip can sweep a hue shift, push a time
warp and fade an effect together, and moving the clip moves all of them.

```text
automation: [
  AutomationClip {
    row: elements.outputs_layout.all_outputs,
    start: 0s,
    duration: 12s,
    curve: [(0.0, 0.0), (1.0, 1.0)],
    bindings: [
      NodeParam { node: time_warp, param: offset_seconds },
      ClipParam { clip: pulse_1, param: revolutions },
    ],
    detached: [],
  },
],
```

Mark collections place beats and cues on the timeline. Mark effects retrigger
on them, and any effect can query them (`mark_last(beats, time)`).

## The controller runs the show

Donder compiles a sequence once, for exactly the controller ports you select,
into a compact archive of prepared data and bytecode. The same portable runtime
evaluates that archive in the preview window, for live E1.31 and Art-Net output,
and on an ESP32. Frame checksums from the controller match the desktop's.

- **Editor playback.** Install the bundled firmware over USB. The controller
  hosts its own Wi-Fi network and advertises itself; add it to the setup, claim
  it and press Play. Each controller receives its own compiled fragment and
  starts at a shared, clock-synchronized time. No per-frame pixels cross the
  network.
- **Standalone playback.** The controller stores the last show in flash and can
  loop it on four parallel WS281x outputs, with no computer attached.
- **Supported today:** classic ESP32 boards with 4 MB flash, including the
  QuinLED Dig-Quad, up to 1,600 pixels per controller. See
  [controller setup](docs/esp32_loading.md#install-from-donder).

## A show is a project folder

A project is plain text in one language: data documents (`*.data.donder`) hold
fixtures, layouts, patches, controllers, curves, gradients and sequences, and
scripts (`*.donder`) hold effects and operators. It lives happily in git.
Documents import each other, so a fixture definition, gradient or sequence can
be shared across shows or kept inside one file.

The graphical editors and the text editor work on the same project, and they
correspond exactly: every data document is one editor state and every editor
state is one document. Edit a fixture's shapes on the canvas, route pixel ranges
to controller ports, or type the document directly, with diagnostics that point
at the exact token. Every graphical edit is undoable, and saving prints the
canonical text, so it never changes a token you wrote.

`examples/stanford_room` is a complete five-and-a-half-minute room show at 120 fps that
has played standalone on a Dig-Quad. `examples/starter` is a smaller project
with 30 outputs, sample sequences, and every bundled effect and operator.

## Status

Donder is pre-release and moving fast. Project and archive formats change
without migrations until there is a deliberate compatibility commitment.

## Getting started

Install the Rust toolchain from `rust-toolchain.toml`, the Node.js and pnpm
versions required by `package.json`, and the
[Tauri 2 system dependencies](https://tauri.app/start/prerequisites/) for your
platform. Then:

```bash
pnpm install
pnpm tauri dev
```

Open `examples/starter/project.data.donder`, or follow
[Create your first LED show](docs/first_show.md) to build a two-pixel show from
an empty project, preview it and route its output.

A command-line tool checks and copies projects:

```bash
cargo run -p donder-cli -- --path examples/starter check
```

`donder lsp` runs the language server over stdio for other editors.

## Development

```text
crates/donder-language/         Domain types, the effect language compiler and bytecode
crates/donder-project-io/       Project loading, imports, diagnostics and saving
crates/donder-elaboration/      Preparing a sequence for selected outputs
crates/donder-runtime/          Portable no_std playback runtime and archive format
crates/donder-preview/          Preview playback and wgpu renderer
crates/donder-output/           E1.31 and Art-Net transports
crates/donder-language-server/  Language server for documents and scripts
crates/donder-cli/              Command-line project tools
crates/donder-editor/           Shared GUI projection and edits for desktop and browser
crates/donder-sequence-api/     Shared editor DTOs and generated TypeScript types
crates/donder-browser/          WASM editing and playback session for the website
apps/desktop/                   Tauri desktop app: Rust service and React frontend
firmware/esp32/                 ESP32 controller firmware (separate Cargo workspace)
examples/                       The starter project and the Stanford room show
docs/                           User guides and reference
```

Before submitting changes, run:

```bash
cargo fmt
pnpm check
```

`pnpm check` regenerates the TypeScript bindings, then runs frontend typechecking,
lint, unused-export analysis, tests and build, and Rust formatting, tests,
device-storage tests and Clippy. The device-storage tests need a host C compiler
and native libclang. Set `DONDER_HOST_LIBCLANG_PATH` to the library file or its
directory:

| Platform | Install | Path |
| --- | --- | --- |
| Windows | `winget install LLVM.LLVM` | Your LLVM `bin` directory, commonly `C:\Program Files\LLVM\bin` |
| macOS | `brew install llvm` | The `lib` directory under `brew --prefix llvm` |
| Debian/Ubuntu | `apt install libclang-dev` | The LLVM `lib` directory containing `libclang.so` |

`apps/desktop/frontend/src/generated/bindings.ts` and `apps/desktop/gen/schemas/`
are generated: use `pnpm generate:bindings` and the Tauri tooling, not hand
edits. Benchmarks use Criterion (`pnpm bench:effect-vm:quick`, or
`pnpm bench:effect-vm` for the full set). Firmware builds need the ESP Rust
toolchain; see the [firmware README](firmware/esp32/README.md).

[The documentation](docs/README.md) covers the [architecture](docs/architecture.md),
[project language](docs/project_language.md) and [effect language](docs/effect_language.md).
