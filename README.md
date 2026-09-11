# Dawn

Dawn is a desktop workbench for authoring programmable light shows as source-controlled projects. It combines an IDE-style editor, project validation, timeline-oriented sequencing, real-time preview rendering, and export tooling for show files.

The project is built as a Rust workspace with a Tauri desktop shell and a React/TypeScript frontend. The core model, Dawn document loading, effect DSL, and renderer live in Rust so project validation and frame rendering share the same typed domain model.

## Why This Exists

Lighting tools often split creative sequencing from the source data that makes a show maintainable. Dawn stores reusable pixel fixture definitions, layouts, LED routes, controllers, effects, sequences, and audio references as Dawn source documents, checked together and edited through text and GUI workflows.

That makes the project useful as a technical showcase for:

- A typed Rust domain model for a non-trivial creative tool.
- A custom effect DSL with parsing, type checking, compilation, and VM execution.
- A real-time renderer for pixel-based lighting sequences.
- A desktop application architecture that keeps frontend UI state synchronized with Rust-owned project state.
- Practical editor features such as diagnostics, project trees, generated TypeScript bindings, and example projects.

## Features

- Open and validate Dawn project files.
- Edit project documents in a CodeMirror-based desktop editor.
- Compose fixture definitions from pixels and other definitions, place instances in layouts, group them for effects, and route RGB/RGBW output to controllers.
- Render one shared logical/controller frame through the Rust runtime.
- Preview effect rasters and sequence output in the desktop UI.
- Transmit live E1.31 or Art-Net output with blackout and stream lifecycle handling.
- Install bundled ESP32 firmware over USB, configure Wi-Fi, and upload a sequence
  for persistent standalone playback. See [controller setup](docs/esp32_loading.md#install-from-dawn)
  for supported hardware and the current verification limits.
- Generate TypeScript bindings from Rust command and data types.
- Resolve, cache, inspect, pack, publish, fork, and template Dawn packages.
- Benchmark effect VM and render performance with Criterion.

## Tech Stack

- Rust 2024 workspace
- Tauri 2 desktop runtime
- React, TypeScript, and Vite frontend
- CodeMirror editor integration
- wgpu-backed rendering infrastructure
- Criterion benchmarks
- pnpm workspace tooling

## Repository Layout

```text
apps/desktop/                 Tauri desktop app
apps/desktop/src/             Rust desktop service, app state, commands, persistence
apps/desktop/src/desktop_state/ Desktop audio, workspace, GUI edit, project, render, and filesystem workflows
apps/desktop/src/gui/         Typed GUI projection, edit, selection, and domain-conversion modules
apps/desktop/src/state_tasks/ Background save/render scheduling and GUI history
apps/desktop/src/preview/geometry.rs Read-only preview-prop geometry projection
apps/desktop/frontend/        React/TypeScript frontend
apps/desktop/frontend/src/ui/gui/sequence/sequenceWaveform.ts  Timeline waveform cache/rendering
crates/dawn-language/         Dawn authoring model and effect/operator compiler
crates/dawn-runtime/          Portable no_std bytecode VM and sequence evaluation core
crates/dawn-device-storage/   Portable device records, credentials, and flash recovery tests
crates/dawn-elaboration/      Host-side generator expansion, lowering, and output preparation
crates/dawn-package/          Manifest v2, resolution, locks, cache, registry protocol, packing
crates/dawn-project-io/       Dawn project loading, diagnostics, source ownership, save/export
crates/dawn-project-io/src/loader/  Project loading, import resolution, and document parsing
crates/dawn-project-io/src/serialization/  Domain-specific Dawn document serialization
crates/dawn-output/           E1.31 and Art-Net socket/codec lifecycle
crates/dawn-cli/              Standalone `dawn` package and project CLI
firmware/esp32/               ESP32 loader, Wi-Fi transport, parallel I2S output, and profiling harness
examples/                     Example Dawn projects and props
docs/                         Architecture, loading, performance, and regression notes
tools/                        Repository tooling
```

## Getting Started

To use the app, follow [Your first show](docs/first_show.md) for a two-prop project,
preview playback, output assignment, and saving without editing YAML.
To customize a show that imports packages, choose **File → Create Editable
Project Copy...**. Dawn opens a separate project containing the show, imported
definitions, and referenced audio as editable project files.

### Prerequisites

Install:

- Rust toolchain from `rust-toolchain.toml`
- Node.js version required by `package.json`
- pnpm version pinned in `package.json`
- Tauri 2 system dependencies for your operating system
- A host C compiler and host libclang for the device-storage workspace tests.
  Set `LIBCLANG_PATH` to the host LLVM library when it is not discoverable.
  The ESP cross-toolchain libclang is for firmware builds, not host tests.

### Install Dependencies

```bash
pnpm install
```

### Run The Desktop App

```bash
pnpm tauri dev
```

This starts the Vite frontend through Tauri and opens the Dawn desktop app.

### Try An Example Project

After the app opens, load the example package manifest:

```text
examples/starter/dawn-package.json
```

`examples/starter` is the complete 30-output starter project, including example effects, gradients, curves, operators, sequences, and audio assets.

## Development Commands

`apps/desktop/frontend/src/generated/bindings.ts` and
`apps/desktop/gen/schemas/` are committed generated API artifacts. Generate
bindings with the command below; generate schemas through Tauri tooling. Do not
edit either by hand.

```bash
pnpm generate:bindings
```

Regenerates TypeScript bindings from the Rust desktop API.

```bash
pnpm check
```

Runs generated bindings, frontend type checking, linting, dead-export analysis, frontend tests/build, and the Rust format, check, test, and Clippy gates.

```bash
cargo fmt
```

Formats the Rust workspace.

```bash
pnpm bench:effect-vm:quick
```

Runs a quick Criterion smoke pass for the effect VM and renderer benchmarks.

```bash
pnpm bench:effect-vm
```

Runs the full Criterion benchmark set.

## ESP32 Firmware

`firmware/esp32` is a separate Cargo workspace for the classic ESP32. It loads
prepared sequences over Wi-Fi and can drive four parallel WS281x outputs with
I2S DMA; it also contains the profiling harness used during runtime work. Its
toolchain and lockfile are isolated from desktop builds. See
[ESP32 loading](docs/esp32_loading.md) for the loader and
[firmware instructions](firmware/esp32/README.md) for build and board commands.

## Package and CLI workflow

Every project and module starts at `dawn-package.json`. The manifest owns the
stable UUID module identity, exact language version, Dawn compatibility range,
optional project entrypoint, explicit exports, alias-keyed dependencies, and
audio declarations. `dawn.lock` pins the registry, exact release versions,
archive hashes, module identities, dependency edges, and path-dependency
content hashes. Opening a project is offline and deterministic; use Sync
explicitly when its lock or cache is missing.

Run the standalone client from the workspace with:

```bash
cargo run -p dawn-cli -- --help
```

It provides `init`, `check`, `add`, `remove`, `sync`, `update`, `tree`, `pack`,
`publish`, `login`, `logout`, `whoami`, `fork`, and `new --from`. Login uses
browser device approval and OS credential storage. Registry artifacts are
downloaded to a content-addressed cache, structurally inspected, compiler
validated in temporary storage, and atomically installed. Desktop package
operations use the same service layer.

`dawn fork <alias>` copies that direct registry dependency into
`modules/<package-name>/`, assigns the copy a new module ID, clears its
publication identity, and replaces the registry requirement with a path
dependency under the same alias. The copied package keeps its own exports,
assets, local imports, and transitive dependencies; project dependency imports
therefore continue to resolve through the same alias and export groups.

## How A Dawn Project Works

The manifest's `project.entrypoint` imports the rest of the show definition:
setups, element trees, preview layouts and props, fixture profiles, patch
graphs, controllers, curves, effects, sequences, and assets. Imports are
structured as module-local document lists or dependency alias/export-group
references; dependency deep imports and root escapes are rejected.

Project IO loads reachable source files, validates imports and references, tracks source locations for diagnostics, compiles DSL definitions, and builds the authoritative typed `DawnProject`. `SourceProject` retains document ownership, import, original-source, and asset metadata; it is not a second editable project model. GUI commands make one private mutable candidate from the current immutable project snapshot; accepted snapshots are shared by state, history, save, and render work. Project IO serializes typed state directly without reparsing or synchronizing a YAML model.

After DSL compilation, `dawn-elaboration` validates the selected setup and sequence, expands generators, resolves targets, and lowers authored graphs into prepared numeric data. `dawn-runtime::sequence::PreparedSequence` is the complete playback artifact: its `signals` field holds a `PreparedSignalGraph`, alongside controls, fixture behavior, and the prepared patch. Create its workspace and output buffers once, then call `sequence.evaluate(time, &mut buffers, &mut workspace)` for each frame. The runtime evaluates logical colors, applies controls and fixture behavior, and executes the patch into those buffers. `dawn-runtime::signal::PreparedSignalGraph::evaluate` is the narrower logical-color interface; its `SignalPlan` contains the graph connections and preassigned buffer/VM schedule. Workspace creation reserves reusable VM, automation, array, and patch storage; prepared-frame allocation tests cover the playback hot path. Networking and physical pin timing remain outside the runtime. Preview and live output consume the same `RenderedSequenceFrame`; neither reinterprets colors, fixture channels, or patch ordering. Live output is opt-in for each application run and fails closed by blacking out active ports and terminating E1.31 streams.

Use `PreparedSequenceOutput::prepare_selected` to prepare a compact sequence for selected controller ports. See [output selection](docs/output_selection.md) for the API, preserved sampling semantics, and measured memory reductions.

The sequence-as-code validity rules, curve semantics, parser behavior, and runtime
budgets are documented in [the sequence-as-code contract](docs/sequence_as_code.md).

## Status

Dawn is an active prototype. The codebase emphasizes fast iteration, typed state, explicit validation, and a single project model shared by the editor, GUI workflows, and renderer.
