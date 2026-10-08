# Donder

Donder is a light-show sequencer where every effect is a program, fast enough
that a whole show plays live on an ESP32.

<!-- TODO: GIF or short video of examples/stanford_room, then an editor screenshot. -->

Most sequencers render a show into a big file of pre-computed frames before it
can play. Donder compiles effects to compact bytecode for a virtual machine
built for lighting, and evaluates every frame live. A ten-year-old laptop
renders 50,000 pixels at 60 fps<!-- TODO: CPU, year, link to measurement -->,
faster than xLights renders the same show and with a fraction of the
memory<!-- TODO: measured ratio and artifact in docs/performance.md -->. That
headroom lets the 5½-minute, 144 fps [Stanford room show](examples/stanford_room)
run on a QuinLED Dig-Quad with no computer attached.

You don't need to write code to use Donder. It ships with a library of effects
and operators, and everything has a graphical editor. When you want something
the library doesn't have, you can write it yourself.

## What makes it different

- **Effects are programs.** Every effect, including every bundled one, is a
  short program you can open, change or copy. An effect computes one color per
  pixel per frame from the pixel's position, time, beat marks, curves and
  gradients. → [Effect language](docs/effect_language.md),
  [tutorial](docs/effect_tutorial.md)
- **A layer graph, not a layer stack.** Layers feed a node graph of operators:
  blends, dimming, hue shifts, delays, echoes, mirrors, time warps. Operators
  are written the same way as effects, and they can sample their inputs at any
  time and any pixel. A time warp is a single line:
  `sample { source.at(time + offset_seconds) }`.
  → [Operator inputs](docs/effect_language.md#operator-inputs)
- **Automation like a DAW.** An automation clip is a curve on the timeline that
  can drive any number of effect and operator parameters at once. Beat marks
  retrigger effects, and you can tap them in while the song plays.
  → [Sequences](docs/project_language.md#sequences)
- **The whole show is text.** Fixtures, layouts, patching, controllers and
  sequences are plain documents in one language. They diff cleanly in git and
  import each other, so a fixture, gradient or effect can be shared across
  shows. `donder check` validates a project the way a compiler does. That also
  means an LLM can set up fixtures, patch controllers or edit a sequence and get
  exact errors back. → [Project language](docs/project_language.md)
  <!-- TODO: link the registry website once it is public. -->
- **GUI and code are the same project.** Switch between the graphical editor and
  the text view at any time. Every editor state is a document, every edit can be
  undone, and saving never rewrites a token you typed. A language server gives
  any editor diagnostics, completion and navigation (`donder lsp`).
- **Plays on what you have.** Stream live over E1.31 or Art-Net, export an FSEQ
  file and schedule it in FPP next to your existing shows, or upload to an ESP32
  that plays standalone. The preview, the live output and the controller run
  the same runtime, and their frames match checksum for checksum.
  → [ESP32 controllers](docs/esp32_loading.md)

## Installation

<!-- TODO: prebuilt installers and the browser editor, once available. -->

Donder currently builds from source. Install the Rust toolchain from
`rust-toolchain.toml`, the Node.js and pnpm versions required by `package.json`,
and the [Tauri 2 system dependencies](https://tauri.app/start/prerequisites/)
for your platform. Then:

```bash
pnpm install
pnpm tauri dev
```

Open `examples/starter/project.data.donder` to explore a 30-output project with
sample sequences and every bundled effect and operator, or follow
[Create your first LED show](docs/first_show.md) to build one from scratch.

The command-line tool checks projects and exports FSEQ files:

```bash
cargo run -p donder-cli -- --path examples/starter check
cargo run -p donder-cli -- --path examples/starter export-fseq layer_test show.fseq
```

## Learn more

- [Create your first LED show](docs/first_show.md)
- [Write an effect](docs/effect_tutorial.md)
- [Fixture authoring](docs/fixture_authoring.md): lines, arcs, grids and
  matrices, layouts and groups
- [ESP32 controllers](docs/esp32_loading.md): install, upload and standalone
  playback
- [Performance](docs/performance.md) and [architecture](docs/architecture.md)
- [All documentation](docs/README.md)

## Status

Donder is pre-release and moving fast. Project and archive formats change
without migrations until there is a deliberate compatibility commitment.

Standalone playback supports classic ESP32 boards with 4 MB flash, including the
QuinLED Dig-Quad, with up to 1,600 pixels on four outputs per controller. Larger
displays use live E1.31/Art-Net output or FSEQ export.

Not yet supported:
- Prebuilt installers and the browser editor
- Importing xLights models and layouts
- Automatic beat detection
- Non-pixel DMX fixtures such as moving heads
- Live input (MIDI, OSC, audio). Parameters are evaluated live every frame, so
  live input will drive them through the same path automation uses today.

<!-- TODO: license and community links. -->

## Contributing

See [development](docs/development.md) for the repository layout and the checks
to run before submitting changes.
