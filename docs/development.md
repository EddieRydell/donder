# Development

## Repository layout

```text
crates/donder-runtime-types/    Values, bytecode and prepared inputs that playback accepts (no_std)
crates/donder-language/         The text language: data-document syntax, effect compiler, analysis
crates/donder-model/            Domain model: fixtures, layouts, setups, sequences, validated edits
crates/donder-project-io/       Project loading, imports, diagnostics and saving
crates/donder-elaboration/      Preparing a sequence for selected outputs
crates/donder-runtime/          Portable no_std playback runtime: trusted shows and their evaluation
crates/donder-output/           E1.31 and Art-Net transports, FSEQ export
crates/donder-audio-analysis/   Beat and downbeat detection from song audio
crates/donder-language-server/  Language server for documents and scripts
crates/donder-cli/              Command-line project tools
crates/donder-editor/           Shared GUI projection and edits, and the editor DTOs exported to TypeScript
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

`pnpm check` runs Rust formatting and the workspace tests, built once with
Clippy as the compiler so that strict lints and tests share one build (their
`generated_bindings` tests regenerate the TypeScript bindings), then frontend
typechecking, lint, unused-export analysis, tests and build, and the
device-storage tests. The device-storage tests need a host C compiler
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

openh264 compiles its x86 assembly only when `nasm` is on `PATH`; without it
the build still succeeds but video export encodes several times slower.
`pnpm firmware:build` and `pnpm firmware:cargo` select ESP tools in child
processes using `DONDER_ESP_LIBCLANG_PATH` and `DONDER_ESP_TOOLCHAIN_BIN`.

## Firmware runtime constraints

The runtime is 32-bit: `donder-runtime` and `donder-runtime-types` never use
`u64`, `i64`, `u128`, `i128` or `f64` (their `clippy.toml` bans them), and
`pnpm firmware:build` fails if runtime code calls 64-bit integer or
double-precision routines. Firmware places the strip interpreter and graph
evaluation in IRAM: every function in `dsl/vm/strip.rs` and `evaluation.rs`
carries the `iram`-feature `link_section` attribute, except the
once-per-frame or once-per-clip code in `evaluation::frames`, which stays in
flash. `pnpm firmware:build` fails if any other function lands in flash.
Closures cannot carry the attribute: when the check reports an outlined
closure, move its body into a named `#[inline(never)]` method.

## Dependency maintenance

Dependency freshness is checked locally, not by external update PRs. Inspect
both Cargo workspaces and the pnpm workspace; the ESP32 workspace under
`firmware/esp32` has its own manifest and lockfile and is checked separately.
Use read-only checks: `pnpm outdated`, `pnpm audit`, `cargo outdated
--workspace`, `cargo deny check advisories -W unmaintained`, and `cargo tree -i
<crate>`. For firmware use `--manifest-path firmware/esp32/Cargo.toml` with
`cargo outdated` and `cargo tree`, and run `cargo deny` from `firmware/esp32`
(it takes no `--manifest-path`). The `-W unmaintained` form keeps
vulnerability advisories fatal while printing inherited unmaintained packages.
Check direct and transitive dependencies, including SDK and HAL packages.
Do not update a manifest or lockfile during an inspection. Report separately
available upgrades, security advisories, unmaintained transitive packages,
upstream Git patches, and intentionally pinned hardware SDK revisions, and
summarize before making changes.

See [testing and benchmarks](testing.md) for where tests live and the
benchmark workflow, and [architecture](architecture.md) for how the crates fit
together.
