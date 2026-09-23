# Donder ESP32 firmware

This standalone Cargo workspace owns the Xtensa target, ESP SDK dependencies,
device persistence, profiling harnesses, and the Wi-Fi/I2S loader. Keeping it
separate prevents embedded-only build scripts and target configuration from
breaking the host workspace. Its local `crates/donder-device-storage` crate owns
LittleFS credential and archive storage.

The firmware consumes `donder-runtime` prepared sequences. Source parsing,
imports, generator expansion, target resolution, and controller selection remain
host concerns. See [prepared sequence loading](../../docs/esp32_loading.md) for
the archive, transport, install, and verification workflow.

## Toolchain and dependencies

Enter the environment before firmware commands:

```powershell
. ./export-esp.ps1
```

The workspace targets the classic ESP32 using the esp-rs toolchain. SDK crates
are patched together to upstream revision
`0eb3e53b4a2e555d2136ce9dc83e18c6692b9673` because the published radio beta
targets an older HAL. Keep the patched SDK crates on one source and use
`--locked`; remove the patches together only after a compatible published stack
exists and the board checks are repeated.

## Binaries and features

- `donder-esp32` is the standalone benchmark harness.
- `pc_profile` with `pc-profile` records interrupted instruction addresses for
  host-side symbolization. It is not a call-stack profiler.
- `loader` with `loader` enables persistent credentials and Wi-Fi upload.
- `loader` with `i2s-output` additionally runs continuous four-lane WS281x output
  on the second core.

Build the installable image from the repository root:

```powershell
./firmware/esp32/build-image.ps1
```

For focused development builds from this directory:

```powershell
cargo +esp build --release --bin donder-esp32 --locked
cargo +esp build --release --features pc-profile --bin pc_profile --locked
cargo +esp build --release --features i2s-output --bin loader --locked
```

Directly flashing the loader must use `partitions.csv`; omitting it loses the
reserved Donder data layout. Flashing changes the application, bootloader, and
partition table and is never part of a read-only validation run.

## Capture policy

`capture.py`, `capture_pc.py`, and `upload.py` reject incomplete records,
duplicate measurements, checksum mismatches, and corrupt serial data. Never
repair a partial capture by deleting bad bytes. Preserve the matching ELF until
host symbolization and hash recording are complete.

Write ordinary output below `target`, not `results`. Only a complete, reviewed,
minimal capture supporting a durable claim is promoted to `results/accepted`.
See the [evidence policy](../../docs/performance.md) and the
[accepted evidence index](results/README.md).

## Validation

The root `pnpm check` runs all six device-storage recovery tests through
`pnpm storage:test`. Run that command from the repository root: it uses the
host Rust toolchain and does not inherit this directory's Xtensa Cargo config.
These tests require a host C compiler and host libclang. On Windows, set
`LIBCLANG_PATH` to a host LLVM library before running the gate; do not use the
ESP cross-toolchain libclang selected by `export-esp.ps1`. A wrong library can
make bindgen fail with a host pointer-size assertion. This machine also has a
usable host library bundled with RStudio; its location is machine configuration,
not a repository dependency.

The desktop and ESP32 workspaces have different toolchains. Do not run host
builds with `+esp`. From this directory, firmware validation is:

```powershell
cargo +1.98.1 fmt --check
cargo +esp clippy --release --features pc-profile --bins --locked -- -D warnings
cargo +esp clippy --release --features i2s-output --bin loader --locked -- -D warnings
uvx --from esptool python -m unittest test_capture_pc
```

These checks do not flash a board or verify physical LED output. Report source
checks, on-device frame checksums, deadline measurements, DMA completion, and
physical electrical/LED validation as separate boundaries.
