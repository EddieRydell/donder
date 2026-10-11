# Testing and benchmarks

## The gate

```bash
cargo fmt
pnpm check
```

`pnpm check` runs, in order:
1. `cargo fmt --check` and the workspace Rust tests, whose `generated_bindings`
   tests write the committed TypeScript bindings;
2. frontend typechecking, lint, unused-export analysis, tests and production build;
3. the firmware-owned device-storage tests on the host (`pnpm storage:test`);
4. strict workspace Clippy.

The storage tests need a host C compiler and `DONDER_HOST_LIBCLANG_PATH`; see
the root [development setup](../README.md#development). They build in
`target/storage-host`. Firmware code outside device storage is checked with the
commands in the [firmware README](../firmware/esp32/README.md#validation).

Documentation and example-only changes do not need the gate.

The gate's host Cargo commands all build the whole workspace, so they share one
set of artifacts. A `-p` build resolves a different feature set (`donder-browser`
adds `syn` and `donder-model` features), which changes most dependency artifacts
and rebuilds them.

The dev profile compiles workspace crates at opt-level 1 and dependencies at
opt-level 2. Tests load and compile whole projects, and unoptimized builds made
them many times slower.

## Where tests live

- Integration tests are in `crates/*/tests`, unit tests beside their modules, and
  desktop service tests beside the desktop workflows. `donder-project-io`'s
  integration tests form one binary, `tests/project_io`, around their shared
  helpers.
- `examples/starter` is the fixture for realistic project flows. Invalid or
  synthetic projects are written to temporary directories in the test that needs
  them.
- Shared playback workloads are in the dev-only `donder-test-support` crate,
  used by the runtime integration tests and both benchmarks. Runtime unit tests
  use it only for helpers whose values are language types; a test that builds
  a prepared sequence with it belongs under `crates/donder-runtime/tests`.

Key contracts and their tests:

| Contract | Tests |
| --- | --- |
| Save and reload preserve meaning | `donder-project-io` `semantic_preservation`, `roundtrip`, `path_refactor` |
| Strict parsing with exact locations | `donder-project-io` `schema_strictness`, `diagnostics` |
| Controller fragments match full playback | `donder-elaboration` `output_selection` |
| Archive round trips and corruption rejection | `donder-elaboration` `sequence_archive`, `clip_sampling`; `donder-runtime` `archive` |
| Only well-formed programs are admitted | `donder-language` `admission` |
| Any source compiles or reports diagnostics, without panics or stack overflow | `donder-language` `compiler_totality` |
| Language semantics and diagnostics | `donder-runtime` `dsl`, `arrays`, `event_queries`, `hsv_intrinsics` |
| Preparation folds, slots and stages the right work | `donder-runtime` `optimization` |
| Playback does not allocate | `donder-runtime` `playback_allocations`, `donder-elaboration` `controller_allocations` |
| A strip computes every pixel as that pixel alone | `donder-runtime` `strips` |
| Staged, fused and cached execution match plain sampling | `donder-runtime` `prepared_uniform`, `staged_execution`, `fusion`, `dsl_temporal`, `spatial_signal` |
| Black inputs fold away during preparation | `donder-elaboration` `standard_operators` |
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
