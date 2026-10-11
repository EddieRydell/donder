# Development

## Repository layout

```text
crates/donder-runtime-types/    Values, bytecode and prepared inputs that playback accepts (no_std)
crates/donder-language/         The text language: data-document syntax, effect compiler, analysis
crates/donder-model/            Domain model: fixtures, layouts, setups, sequences, validated edits
crates/donder-project-io/       Project loading, imports, diagnostics and saving
crates/donder-elaboration/      Preparing a sequence for selected outputs
crates/donder-runtime/          Portable no_std playback runtime and archive format
crates/donder-output/           E1.31 and Art-Net transports, FSEQ export
crates/donder-audio-analysis/   Beat and downbeat detection from song audio
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

## Checks

Before submitting changes, run:

```bash
cargo fmt
pnpm check
```

`pnpm check` runs Rust formatting and the workspace tests, whose
`generated_bindings` tests regenerate the TypeScript bindings, then frontend
typechecking, lint, unused-export analysis, tests and build, device-storage tests
and Clippy. The device-storage tests need a host C compiler
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
toolchain; see the [firmware README](../firmware/esp32/README.md).

See [testing and benchmarks](testing.md) for where tests live and the
benchmark workflow, and [architecture](architecture.md) for how the crates fit
together.
