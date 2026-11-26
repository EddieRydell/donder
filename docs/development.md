# Development

## Repository layout

```text
crates/                         The workspace crates; each one's job is in the architecture crate table
apps/desktop/                   Tauri desktop app: Rust service and React frontend
firmware/esp32/                 ESP32 controller firmware (separate Cargo workspace)
examples/                       The starter project, the Stanford room show and the Rydell house show
docs/                           User guides and reference
```

## Checks

The gate, `cargo fmt` then `pnpm check`, is described in
[testing](testing.md#the-gate). Its device-storage tests need a host C compiler
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

## Errors

Every error a person reads says what failed and where. There are three
channels, chosen by what the error is about:

- **Project problems** are `IoDiagnostic`s: a document path, a text range
  where one exists, a severity, a code and a message. Loading, syntax,
  scripts, project rules and referenced assets all report this way, and the
  Problems view, the language server and the CLI read the same records. A
  diagnostic about something a document references, such as a missing asset
  warning, sits at the referencing field, not at the referenced file.
  Positions count UTF-16 code units, as LSP and Monaco do.
- **Refused GUI edits** are `GuiProblem`s, at the field of the open editor
  the model's problem site names. They are built from typed state; an edit
  never prints document text to locate its problem.
- **Activity failures** (export, output, device, audio playback, preview,
  render) are messages shown at the control that ran them. Their error types
  implement `Display` with a plain sentence that names the file, controller
  or device; never show an error's `Debug` form.

States validation excludes are not errors. Code that holds a validated
project or a `View` never reports a missing reference or an impossible value:
the view or accessor that relies on the invariant states it once, as
`unreachable!("<the invariant that proves it>")`, and call sites use its
infallible result. Only input from outside, such as an id in a request, is
checked, where it enters.

Filesystem calls go through `fs_err`, whose errors name the path and the
operation; the root `clippy.toml` bans the `std::fs`, `Path` and `camino`
calls it replaces.

## Firmware runtime constraints

The runtime is 32-bit: `donder-runtime` and `donder-runtime-types` never use
`u64`, `i64`, `u128`, `i128` or `f64` (their `clippy.toml` bans them), and
`pnpm firmware:build` fails if runtime code calls 64-bit integer or
double-precision routines. Firmware places the strip interpreter and graph
evaluation in IRAM: every function in `dsl/vm/strip.rs` and `evaluation.rs`
carries `#[iram]` (`donder-runtime-iram`; on an impl it covers every
function), which links it into `.rwtext` under the `iram` feature, except the
once-per-frame or once-per-clip code in `evaluation::frames`, which stays in
flash. `pnpm firmware:build` fails if any other function lands in flash.
Closures cannot carry the attribute: when the check reports an outlined
closure, move its body into a named `#[inline(never)]` method.

## Builds

Every host Cargo command shares one build per profile: the tests, `pnpm tauri
dev`, `pnpm generate:bindings` and `cargo run -p donder-cli` reuse each
other's workspace crates. Two things keep it that way:

- `donder-workspace-hack` makes every workspace crate see the same dependency
  features, so building one package resolves the same feature set as the whole
  workspace. After changing a dependency or its features, run
  `cargo hakari generate` and `cargo hakari manage-deps` (install with
  `cargo install cargo-hakari --locked`); `pnpm check` fails while either has
  work to do. The firmware's `no_std` crates (`donder-runtime`,
  `donder-runtime-types`, `donder-device-protocol`) do not depend on it, per
  `.config/hakari.toml`.
- Clippy runs as `cargo clippy`, which only checks, rather than as the compiler
  of the test build. A compiler wrapper or `RUSTFLAGS` set for one command
  gives it a separate build that no other command reuses.

Cargo never removes the outputs of superseded builds; run `cargo clean` when
`target/` grows large.

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

Upgrades land together in one pass, not alongside other work: a lockfile
change to a widely used crate (`serde`, `syn`, `proc-macro2`, `windows`,
`tauri`) gives everything downstream a new hash and costs a near-cold build.
Adding a dependency changes only its own entries (`cargo update -p <crate>`),
never a bare `cargo update`, and is followed by `cargo hakari generate` and
`cargo hakari manage-deps`.

See [testing and benchmarks](testing.md) for where tests live and the
benchmark workflow, and [architecture](architecture.md) for how the crates fit
together.
