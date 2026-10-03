# Donder

Donder is a desktop workbench for authoring programmable light shows as source-controlled projects. It combines an IDE-style editor, project validation, timeline-oriented sequencing, real-time preview rendering, and export tooling for show files.

The project is built as a Rust workspace with a Tauri desktop shell and a React/TypeScript frontend. The core model, Donder document loading, effect DSL, and renderer live in Rust so project validation and frame rendering share the same typed domain model.

## Why This Exists

Lighting tools often split creative sequencing from the source data that makes a show maintainable. Donder stores reusable pixel fixture definitions, layouts, LED routes, controllers, effects, sequences, and audio references as Donder source documents, checked together and edited through text and GUI workflows.

That makes the project useful as a technical showcase for:

- A typed Rust domain model for a non-trivial creative tool.
- A custom effect DSL with parsing, type checking, compilation, and VM execution.
- A real-time renderer for pixel-based lighting sequences.
- A desktop application architecture that keeps frontend UI state synchronized with Rust-owned project state.
- Practical editor features such as diagnostics, project trees, generated TypeScript bindings, and example projects.

## Features

- Open and validate Donder project files.
- Edit project documents in a CodeMirror-based desktop editor.
- Compose pixel fixture definitions, place reusable instances in layouts, group them for effects, and route RGB/RGBW output to controllers.
- Evaluate preview and controller output through the same portable Rust runtime.
- Preview effect rasters and sequence output in a native GPU window.
- Transmit live E1.31 or Art-Net output with blackout and stream lifecycle handling.
- Install bundled ESP32 firmware over USB, configure Wi-Fi, and upload a sequence
  for persistent standalone playback. See [controller setup](docs/esp32_loading.md#install-from-donder)
  for supported hardware and the current verification limits.
- Generate TypeScript bindings from Rust command and data types.
- Share and customize ordinary local source files and project folders.
- Benchmark effect VM and render performance with Criterion.

## Tech Stack

- Rust 2024 workspace
- Tauri 2 desktop runtime
- React, TypeScript, and Vite frontend
- CodeMirror editor integration
- winit/wgpu native Preview with a portable renderer core
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
crates/donder-language/         Portable language values/bytecode, host authoring model and compiler
crates/donder-runtime/          Prepared-sequence playback and archive codec; private no_std VM
crates/donder-preview/          Portable Preview playback, scene, and wgpu renderer core
crates/donder-elaboration/      Sequence selection, lowering, and output preparation
crates/donder-project-io/       Donder project loading, diagnostics, source ownership, save/export
crates/donder-project-io/src/loader/  Project loading, import resolution, and document parsing
crates/donder-project-io/src/serialization/  Domain-specific Donder document serialization
crates/donder-output/           E1.31 and Art-Net socket/codec lifecycle
crates/donder-cli/              Local project initialization, checking, and copying
firmware/esp32/               ESP32 workspace, device storage, Wi-Fi transport, I2S output, and profiling
examples/starter/             The single maintained example project
docs/                         Current user, architecture, loading, and validation references
```

## Getting Started

To use the app, follow [Your first show](docs/first_show.md) for a two-prop project,
preview playback, output assignment, and saving without editing YAML.
To copy a show into a separate folder, choose **File > Create Standalone Project
Copy...**. Donder opens a project containing the loaded definitions and referenced
audio as editable local files.

### Prerequisites

Install:

- Rust toolchain from `rust-toolchain.toml`
- Node.js version required by `package.json`
- pnpm version pinned in `package.json`
- Tauri 2 system dependencies for your operating system
- A host C compiler and host libclang for `pnpm storage:test`, included in
  `pnpm check`. Install native LLVM/libclang and set `DONDER_HOST_LIBCLANG_PATH`
  in your development environment to its library file or containing directory.
  This is a machine-specific setting; do not commit an installation path.

Typical host LLVM setup:

| Platform | Install | Set `DONDER_HOST_LIBCLANG_PATH` to |
| --- | --- | --- |
| Windows | `winget install LLVM.LLVM` | Your LLVM `bin` directory, commonly `C:\Program Files\LLVM\bin` |
| macOS | `brew install llvm` | The `lib` directory under `brew --prefix llvm` |
| Debian/Ubuntu | `apt install libclang-dev` | The installed LLVM `lib` directory containing `libclang.so` |

For example, in a Windows PowerShell session use
`$env:DONDER_HOST_LIBCLANG_PATH = 'C:\Program Files\LLVM\bin'`; in a POSIX shell
use `export DONDER_HOST_LIBCLANG_PATH=/path/to/llvm/lib`. Configure the same
variable in CI's environment when running the gate.

`pnpm storage:test` requires this setting and passes it as `LIBCLANG_PATH` only
to its Cargo child process, overriding any inherited ESP LLVM selection.
Host storage build artifacts live in `target/storage-host`. The command uses
Node already required by pnpm and runs unchanged on Windows, macOS, and Linux.

### Install Dependencies

```bash
pnpm install
```

### Run The Desktop App

```bash
pnpm tauri dev
```

This starts the Vite frontend through Tauri and opens the Donder desktop app.

### Try An Example Project

After the app opens, load the example project:

```text
examples/starter/project.donder
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

## Local files and CLI

A project folder starts at the fixed root `project.donder` document. Its `workspace`
block contains `format_version` and a stable `project_id` UUID, followed by imports
and authored objects. It has no separate manifest, dependency list, asset inventory,
or lockfile.
All imports point to files inside that folder. Download files or extract a folder
from the registry, then edit imports and sequence targets to fit your show.
Donder does not automatically download other resources or resolve versions.

```bash
cargo run -p donder-cli -- --help
cargo run -p donder-cli -- --path examples/starter check
```

The CLI provides `init`, `check`, and `copy <destination>`. `init` creates the
workspace metadata inside an existing root `project.donder`; `check` validates reachable documents;
`copy` creates an independent project with its loaded sources and referenced audio.
Unreferenced files remain available in the explorer without blocking the active
show. Missing imports or incompatible targets in reachable documents produce
local diagnostics and remain repairable in the text editor.

## How A Donder Project Works

The root `project.donder` imports the rest of the show definition:
setups, layouts, pixel fixture definitions, LED patches, controllers, curves,
gradients, effects, operators, sequences, and assets. Imports are
explicit project-relative document lists; paths escaping the project are rejected.

Project IO loads reachable source files, validates imports and references, tracks source locations for diagnostics, compiles DSL definitions, and builds the authoritative typed `DonderProject`. `SourceProject` retains document ownership, import, original-source, and asset metadata; it is not a second editable project model. GUI commands make one private mutable candidate from the current immutable project snapshot; accepted snapshots are shared by state, history, save, and render work. Project IO serializes typed state directly without reparsing or synchronizing a YAML model.

After loading and DSL compilation, `donder_elaboration::prepare(&project, &sequence_id, outputs)` resolves the selection against the project's active setup, resolves targets, and assembles accepted inputs into prepared numeric data. It returns `Option<PreparedSequence>`; `None` denotes an unresolved selection, not an internal preparation failure. `PreparedSequence` owns immutable playback data behind private fields. Call `sequence.into_playback()` once to create runtime-owned scratch and output storage, then `playback.evaluate(time)` for each frame. Evaluation returns borrowed logical colors and packed controller outputs directly, without a `Result` or caller-supplied workspace. The signal graph evaluator is private. Archive decoding trusts a compatible Donder producer's execution invariants; it checks the envelope, checksum, structural archive representation, and resource budgets, not graph or bytecode correctness. Construction reserves reusable VM, automation, array, and output storage, and prepared-frame allocation tests cover the playback hot path. Text loading, checked edits, archive decoding, networking, and physical pin timing remain separate boundaries. See [selected-output preparation](docs/output_selection.md) for the API and retention contract.

The desktop sends the prepared sequence archive, projected fixture geometry, and clock anchors to a dedicated native Preview process when their revisions change. `donder-preview` decodes the same `PreparedSequence` format and evaluates logical fixture colors locally with `donder-runtime`, so per-frame pixels do not cross Tauri IPC. The Preview process owns its winit window and wgpu surface, which also keeps GTK and wgpu from sharing one Wayland surface. Live output evaluates the prepared sequence for controller bytes in the desktop service. Both paths therefore share authored playback semantics while keeping their host-specific presentation and transport work separate. Live output is opt-in for each application run and fails closed by blacking out active ports and terminating E1.31 streams.

Use the same `prepare` function with `PrepareOutputs::All`, `Controllers`, or `Ports` for full or selected playback. See [output selection](docs/output_selection.md) for the API, preserved sampling semantics, and measured memory reductions.

The sequence-as-code validity rules, curve semantics, parser behavior, and runtime
budgets are documented in [the sequence-as-code contract](docs/sequence_as_code.md).

## Status

Donder is an active prototype. The codebase emphasizes fast iteration, typed state, explicit validation, and a single project model shared by the editor, GUI workflows, and renderer.
