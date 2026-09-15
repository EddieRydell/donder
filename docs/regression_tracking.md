# Regression Tracking

Dawn uses compile-time checks, focused Rust tests, and Criterion benchmarks to keep refactors honest.
Correctness regressions should fail loudly. Timing regressions are advisory until repeated clean runs
show a stable signal.

## Required Gates

Run the full local gate after code changes:

```powershell
pnpm check
```

This runs generated binding export, TypeScript typecheck, frontend lint and unused-code
analysis, production frontend build, frontend tests, Rust format/check/tests, the
firmware-owned storage recovery tests on the host, and strict workspace Clippy.
Run `pnpm storage:test` from the repository root for the storage tests alone.
They require a host C compiler and host libclang; `LIBCLANG_PATH` must not point
to the ESP cross-toolchain library. See the firmware README for the boundary.

For documentation-only, example-only, or other changes unaffected by build/test checks, `pnpm check`
is not required.

## Benchmark Workflow

Effect DSL VM and real-project renderer benchmarks use Criterion only. Do not add custom benchmark
CLIs, JSON reporters, compatibility shims, or legacy command aliases.

Run a quick benchmark smoke pass:

```powershell
pnpm bench:effect-vm:quick
```

Save a Criterion baseline before optimization work:

```powershell
pnpm bench:effect-vm:save
```

Compare the current working tree with that baseline:

```powershell
pnpm bench:effect-vm:compare
```

Run the full benchmark set when finalizing performance-sensitive changes:

```powershell
pnpm bench:effect-vm
```

Criterion output lives under `target/criterion` and is not committed.

## Benchmark Coverage

The direct VM benches live in `crates/dawn-language/benches/effect_vm_bench.rs`. They use public
Effect DSL APIs:

- `compile_effects`
- `bind_params_cached`
- `sample_bound`
- `generate_bound`

The VM benches cover constant return overhead, curve sampling, branch-heavy scan logic, section
position, smoothstep, enum comparisons, curve clamping, seeded random paths, trigonometry, HSV,
dense mixed arithmetic, marks, target sections, `pick`, arrays, loops, and `timeline.emit`.

The renderer benches live in `crates/dawn-elaboration/benches/render_bench.rs`. They load
`examples/starter/project.dawn`, benchmark renderer preparation, and render frames
`144`, `2088`, `5904`, `9504`, `11520`, `19080`, and `25934`.

Renderer benches assert frame checksums and active effect counts. Update those committed expected
values only when a renderer or Effect DSL behavior change is intentional.

## Focused Benchmarks

Run one VM benchmark by name:

```powershell
cargo bench -p dawn-language --bench effect_vm_bench -- scan_sweep
```

Run one render benchmark by frame:

```powershell
cargo bench -p dawn-elaboration --bench render_bench -- render_frame_9504
```

## Regression Classes

### Project Loading

Risk: import resolution, object indexing, diagnostics, source ranges, project root metadata, or
effect script compilation changes unexpectedly.

Current coverage:

- `pnpm check`
- Rust tests under `crates/*/tests` and desktop service tests where present

When adding future coverage, prefer realistic fixtures from `examples/starter`.
Use temporary directories for invalid or synthetic Dawn documents.

### Document Editing And Serialization

Risk: GUI or service edits reorder unrelated content, drop imports, change generated object keys,
alter lane repair, or mutate YAML text outside the intended domain operation.

Current coverage:

- `pnpm check`
- Focused Rust tests for edit paths where present

GUI edit code must mutate typed domain state only. It must not construct, inspect, or mutate
YAML/text directly, and it must not run project checks or reload from YAML after mutation.
Persistence belongs to the IO save path.

### Desktop State And Frontend Contracts

Risk: command refactors desynchronize generated bindings, raw frontend command invocations bypass
typed wrappers, app state transitions change dirty/conflict behavior, or frontend logic changes
selection, preview, terminal, or editor behavior without compiler errors.

Current coverage:

- generated binding export in `pnpm check`
- TypeScript typecheck
- ESLint
- production frontend build
- Rust check and clippy for desktop targets

Generated bindings are generated assets. Do not hand-edit them or reintroduce stale generated
bindings.

### Rendering And Output Runtime

Risk: renderer refactors change output colors, active effect indexing, generated child expansion,
target preparation, timeline buckets, bytecode preparation, or frame timing.

Current coverage:

- `pnpm check`
- `pnpm bench:effect-vm:quick`
- Criterion render checksums and active effect counts
- Criterion Effect DSL VM sample and generator assertions

Treat checksum or active-effect-count changes as behavior changes unless deliberately explained.
Treat benchmark timing as a signal to investigate, not as a hard failure.

### Preview And Audio Sync

Risk: preview refactors render stale frames, reschedule too often, miss frame boundaries, lose
effect-preview filtering, or desynchronize from native audio clock behavior.

Current coverage:

- `pnpm check`
- renderer benchmark checksums for real project frames

Future coverage should prefer headless traces of source refresh, play, pause, seek, rewind,
effect-preview selection, native-audio tick, and frame-boundary behavior.

## Refactor Policy

- Keep behavior-preserving refactors separate from behavior changes.
- Prefer one subsystem at a time: project IO, document edits, desktop state, rendering, preview, or
  frontend.
- Run `pnpm check` for code changes.
- For performance-sensitive Effect DSL or renderer work, save a Criterion baseline before edits and
  compare after edits.
- Do not add fallbacks, shims, or compatibility layers to hide regressions. Fail clearly when
  contracts break.
