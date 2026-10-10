# Contributing to Donder

Thanks for helping. Donder is pre-release software with one main maintainer, so
these rules exist to keep the codebase small, consistent and easy to review.
Read [development](docs/development.md) for setup and
[architecture](docs/architecture.md) for how the crates fit together.

## Pull requests

- Open every pull request as a **draft**. Mark it ready for review only after
  `cargo fmt` and `pnpm check` pass locally and you have fixed every issue you
  know of. Never open a pull request directly as ready for review.
- `pnpm check` is the CI. There is no hosted CI, so run it yourself and say in
  the description that it passed. Do not skip hooks or relax lints to get a
  pass.
- You don't need to open an issue first. Open a draft pull request instead.
- Keep one concern per pull request. Avoid drive-by edits to unrelated code,
  lockfiles, IDE files or generated assets.
- Describe what changed and why. If you changed behavior, say how you verified
  it.
- AI-assisted contributions are welcome, but you are responsible for every
  line. Read and understand the change before you submit it; unreviewed agent
  output will be closed.

## What not to build

- **No LLM, agent or AI integration in the app.** Donder is meant to be driven
  by an ordinary command-line agent with no integration. The `donder` CLI and
  the documentation are that interface.
- **No telemetry, accounts or cloud services.** The app works fully offline;
  network traffic is limited to show output.
- **No native code generation for effects.** Effects stay VM bytecode, and
  playback speed comes from the VM and from preparation.

## Dependencies

Avoid adding new dependencies. If one is clearly worth it, use its latest
compatible release and explain in the pull request why the existing code and
dependencies can't do the job. The maintainer decides whether it stays.

## Code

- **Prefer deletion.** Simplify or extend what exists before adding new code,
  abstractions or modules. Fixes should be general, not special cases for one
  effect or operator.
- **One way to do things.** Keep a single source of truth for each concept, and
  put code in the crate that owns it (see
  [architecture](docs/architecture.md)).
- **No compatibility layers.** Donder has no users to stay compatible with yet.
  When a format changes, update the implementation, fixtures and documentation
  in place. Do not add migrations, legacy aliases, shims or version ranges.
- **No fallbacks.** When something fails, report the error rather than hiding
  it behind a default.
- **Prefer structured data.** Use enums and types for internal logic, not
  strings.
- **Generated files are generated.** Regenerate
  `apps/desktop/frontend/src/generated/bindings.ts` with
  `pnpm generate:bindings` and `apps/desktop/gen/schemas/` with the Tauri
  tooling; never edit them by hand.
- **Frontend styling lives in CSS.** Colors and other static styling values are
  defined in `apps/desktop/frontend/src/styles.css` and reached from runtime
  code through `apps/desktop/frontend/src/theme.ts`.

## Runtime and embedded

The goal is for a whole show to play live on a classic ESP32. When you work on
`donder-runtime`, `donder-runtime-types` or the firmware, keep embedded
constraints in mind: frame time, memory use, IRAM and DRAM, and `no_std`.
Measure representative playback with the benchmarks in
[testing](docs/testing.md) rather than chasing microbenchmark percentages.
Evaluation stays single-core so it remains portable to other hardware.

## Documentation and artifacts

- Documentation describes current behavior. Update it in the same pull request
  as the change, and remove notes the change makes stale.
- Do not commit logs, profiler captures, screenshots, Criterion output or
  implementation journals. Summarize a durable performance result, with its
  date and conditions, in [performance](docs/performance.md).
- Keep scratch files and build output under `target/`. Do not add scratch
  projects under `examples/`.
