# Repository Guidelines

## Project Structure

Rust workspace; the crate map is in `docs/development.md` and the load/compile/prepare/play/edit call paths are in `docs/code_map.md`. Layering: language semantics in `donder-language`, domain rules in `donder-model`, project IO in `donder-project-io`, preparation in `donder-elaboration`, portable frame evaluation in `donder-runtime`, and the values, bytecode and prepared inputs playback accepts in the `no_std` `donder-runtime-types` (an item belongs there only if the runtime consumes it and an upstream crate produces it). Desktop projection code must not reimplement any of them. Firmware lives in `firmware/esp32`, a separate Cargo workspace; keep embedded-only dependencies out of the root workspace.

`examples/starter` is the maintained example; `examples/stanford_room` is the representative performance workload; `examples/rydell_house` is a house display converted from Vixen. The examples' `AGENTS.md` files guide an assistant authoring a show in the Donder language, not work on this codebase; the starter's ships with every new project, so keep them about authoring. Do not add scratch projects under `examples/`.

Every document is written in the Donder project language (`docs/project_language.md`): data documents are `*.data.donder` (root `project.data.donder`), scripts are `*.donder`. The typed `DonderProject` is authoritative after loading; saving prints data documents from typed state with the canonical printer. Do not add a synchronization or typed-to-text mutation phase, per-token provenance, or comment preservation. Data documents and editor states correspond exactly: every field is explicit and canonical, documents have no comments (objects have a `description` field), every name and description the text holds is shown and editable in the GUI, and saving a loaded project rewrites no byte of a canonical document. When adding a field, add it to the document types (`crates/donder-project-io/src/document/types.rs`), the encoder and resolver, and the GUI together.

Every crate's public API is listed in its `lib.rs` (for `donder-language`, the `data`, `compiler` and `analysis` roots); no glob re-exports and no reaching into another crate's modules. Both workspaces enable `unreachable_pub` and `unnameable_types`. Import identity and path remapping belong to project loading and editing, never per-frame evaluation. Mutual document imports are valid: the loader indexes a document's local objects before following imports, so do not reject an in-progress document as a cycle.

Desktop state is split by workflow under `apps/desktop/src/desktop_state`; keep new behavior with its owning workflow instead of growing module roots. Source object kind conversion to `ObjectKind` belongs in the DTO boundary only.

## Testing

Do not add or modify tests unless specifically requested. Prefer `examples/starter` fixtures for realistic flows and temporary directories for invalid or synthetic documents. The gate, test layout and benchmark workflow are in `docs/testing.md`. Prioritize representative playback performance (ESP32 frame times, missed deadlines, memory) over microbenchmarks; do not chase tiny isolated regressions or add complexity to recover a microbenchmark score. Benchmarks use Criterion only. Do not commit Criterion output.

## Agent-Specific Instructions

### Development version policy

Donder is pre-release with no users or compatibility obligations; the product version and the current project/serialization formats are one moving development line (`0.1` / `0.1.0`). Do not add migrations, compatibility layers, legacy aliases, shims, version ranges, or fallbacks; change the current implementation, fixtures and docs in place. Do not add fallbacks when something fails; that hides errors. Keep safeguards that detect malformed data, corruption, impossible values, or an actually incompatible current format.

### Rules

- Never reinvent a solved problem. Use dependencies (after asking the user) rather than the wheel. Before adding one, use its latest compatible release and record any reason for an older one beside it. Dependency inspection procedure: `docs/development.md`.
- Avoid strings in internal logic; prefer enums or structured data.
- Single way of doing things; SSOT; minimize clutter.
- All static frontend styling values (colors, typography, spacing, dimensions, shape, motion, breakpoints, and so on) are defined in `apps/desktop/frontend/src/styles.css` and reach runtime code through `theme.ts`. TypeScript and JSX use only CSS-backed or genuinely runtime/data-dependent values. Reuse existing classes and tokens; add a semantic one when none fits.
- `apps/desktop/frontend/src/generated/bindings.ts` and `apps/desktop/gen/schemas/` are generated; regenerate with `pnpm generate:bindings` and the Tauri tooling, never hand-edit.
- Runtime is 32-bit: `donder-runtime` and `donder-runtime-types` never use `u64`, `i64`, `u128`, `i128` or `f64` (clippy bans them). Every function in the runtime's `dsl/vm/strip.rs` and `evaluation.rs` carries the `iram`-feature `link_section` attribute, except the once-per-frame code in `evaluation::frames`; `pnpm firmware:build` enforces both. Details: `docs/development.md`.
- `DesktopState` owns GUI edit transactionality and history. Snapshots are immutable `Arc<ProjectSession>`; an edit makes one candidate clone and shares the accepted snapshot. Mutate clips through `Arc::make_mut` only when they change; mutation helpers edit the candidate they receive and do not clone the session again.
- Background save/render scheduling uses the single latest-request scheduler in `state_tasks/`; do not add another channel scheduler. Preview frames and MP4 export are drawn only by the Preview's wgpu renderer (`apps/desktop/src/preview/`).
- Keep waveform decoding, caching and drawing in `sequence/sequenceWaveform.tsx`, not `SequenceCanvas.tsx`; pass palette values in rather than duplicating colors.
- GUI edits mutate typed domain state only: never construct, inspect or mutate document text, run project checks, or reload from text after mutation. Persistence belongs to the IO save path.
- Do not use git unless the user requests it; work in the user's checkout and do not create worktrees unless asked.
- Do not use `.env` files.
- Keep temporary scripts, profiling captures and build artifacts under the ignored `target/` directory in this repository. Do not commit raw logs, captures, profiler output, screenshots or implementation journals; record durable results, with date and conditions, in `docs/performance.md`.
- Docs describe current behavior and durable contracts. Consolidate superseded investigation notes into the current reference and remove the journal.
- Avoid unrelated edits to lockfiles, IDE files and generated assets.
- Do not start or leave a frontend dev server running; the user needs `pnpm tauri dev` to own the port.
- Do not jump to editing when the conversation is diagnosing or discussing design. When planning, ask relevant questions and list the files the plan affects.
- After implementing a plan, run `cargo fmt` then `pnpm check` and fix regressions. Not needed for docs, example projects or other changes the checks do not cover.
