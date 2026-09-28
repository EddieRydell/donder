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

Install the ESP Rust toolchain with [espup](https://github.com/esp-rs/espup)
and install `espflash`. Configure these machine-local environment variables:

- `DONDER_ESP_LIBCLANG_PATH`: ESP's libclang file or containing directory.
- `DONDER_ESP_TOOLCHAIN_BIN`: the directory containing `xtensa-esp32-elf-gcc`
  (`xtensa-esp32-elf-gcc.exe` on Windows).

Use the installation locations reported by espup. These vary by platform and
toolchain version; do not commit local paths. Node and pnpm use the root
package's requirements. The firmware runner sets PATH, LIBCLANG_PATH, and
CARGO_TARGET_DIR only for child processes, with build output in this directory's
`target/`. No shell activation is required. Host storage tests separately use
`DONDER_HOST_LIBCLANG_PATH`.

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
pnpm firmware:build
```

For focused development builds from the repository root:

```powershell
pnpm firmware:cargo build --release --bin donder-esp32 --locked
pnpm firmware:cargo build --release --features pc-profile --bin pc_profile --locked
pnpm firmware:cargo build --release --features i2s-output --bin loader --locked
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
These tests require a host C compiler and host libclang. Configure
`DONDER_HOST_LIBCLANG_PATH` as described in the root [prerequisites](../../README.md#prerequisites).
The runner passes that path to Cargo as `LIBCLANG_PATH`, overriding the ESP
selection without modifying the calling shell. It uses `target/storage-host`
under the repository root for host storage artifacts. Missing configuration
fails before Cargo starts; an incompatible library fails during binding generation.

The desktop and ESP32 workspaces have different toolchains. Do not run host
builds with `+esp`. From the repository root, firmware validation is:

```powershell
cargo fmt --manifest-path firmware/esp32/Cargo.toml --check
pnpm firmware:cargo clippy --release --features pc-profile --bins --locked -- -D warnings
pnpm firmware:cargo clippy --release --features i2s-output --bin loader --locked -- -D warnings
uvx --from esptool python -m unittest discover -s firmware/esp32 -p test_capture_pc.py
```

These checks do not flash a board or verify physical LED output. Report source
checks, on-device frame checksums, deadline measurements, DMA completion, and
physical electrical/LED validation as separate boundaries.
