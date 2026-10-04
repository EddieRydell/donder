# Testing and benchmarks

## The gate

```bash
cargo fmt
pnpm check
```

`pnpm check` regenerates TypeScript bindings and runs, in order:
1. frontend typechecking, lint, unused-export analysis, tests and production build;
2. `cargo fmt --check` and the workspace Rust tests;
3. the firmware-owned device-storage tests on the host (`pnpm storage:test`);
4. strict workspace Clippy.

The storage tests need a host C compiler and `DONDER_HOST_LIBCLANG_PATH`; see
the root [development setup](../README.md#development). They build in
`target/storage-host`. Firmware code outside device storage is checked with the
commands in the [firmware README](../firmware/esp32/README.md#validation).

Documentation and example-only changes do not need the gate.

The dev profile compiles workspace crates at opt-level 1 and dependencies at
opt-level 2. Tests load and compile whole projects, and unoptimized builds made
them many times slower.

## Where tests live

- Integration tests are in `crates/*/tests`, unit tests beside their modules, and
  desktop service tests beside the desktop workflows.
- `examples/starter` is the fixture for realistic project flows. Invalid or
  synthetic projects are written to temporary directories in the test that needs
  them.
- Shared runtime workloads are in `crates/donder-runtime/tests/support/`. The
  runtime tests and the render benchmark include them.

Key contracts and their tests:

| Contract | Tests |
| --- | --- |
| Save and reload preserve meaning | `donder-project-io` `semantic_preservation`, `roundtrip`, `path_refactor` |
| Strict parsing with exact locations | `donder-project-io` `schema_strictness`, `diagnostics` |
| Controller fragments match full playback | `donder-elaboration` `output_selection` |
| Archive round trips and corruption rejection | `donder-elaboration` `sequence_archive`, `clip_sampling` |
| Playback does not allocate | `donder-runtime` `playback_allocations`, `donder-elaboration` `controller_allocations` |
| Staged, fused and cached execution match plain sampling | `donder-runtime` `prepared_uniform`, `staged_execution`, `fusion`, `dsl_temporal` |
| GUI edits, history and save | desktop `authoring_acceptance`, `working_copy`, `sequence_rows_acceptance` |
| Starter frame checksums | `donder-elaboration` `starter_sequence_behavioral_checksums...`, `led_output` |

Checksum and active-effect-count changes are behavior changes. Update the expected
values only for an intentional renderer or language change, and say why.

## Benchmarks

Benchmarks use Criterion only; output goes to `target/criterion`.

```bash
pnpm bench:effect-vm:quick     # smoke pass
pnpm bench:effect-vm:save      # baseline before optimization work
pnpm bench:effect-vm:compare   # compare against that baseline
pnpm bench:effect-vm           # full set
```

- **`prepared_playback_bench`** (`donder-runtime`): ScanSweep, ImpactBurst,
  SparkleComet and ShimmerField on four 512-pixel sequences, timed through
  `SequencePlayback::evaluate`. That includes scheduling, VM execution,
  composition and output encoding.
  Focused run: `cargo bench -p donder-runtime --bench prepared_playback_bench -- prepared_effect_suite_4x512_pixels`.
- **`render_bench`** (`donder-elaboration`): preparation of the starter project,
  then representative frames 8398, 8450, 8494, 8530, 9270, 9504 and 9650, plus
  layered, operator and mark workloads. Render-only cases drop output routes but
  keep fixtures. These benches assert frame checksums and active effect counts.
  Focused run: `cargo bench -p donder-elaboration --bench render_bench -- controller_output_dense_60_frames`.

Timings are advisory. Device frame times decide; see [performance](performance.md).
