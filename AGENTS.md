# Repository Guidelines

## Project Structure & Module Organization

This is a Rust workspace. What playback accepts and what it means (values, bytecode, program and invocation descriptions, prepared inputs, sampling math) lives in the `no_std` crate `crates/donder-runtime-types`; the text language (data-document syntax, the effect compiler, editor analysis, import syntax and naming rules) lives in `crates/donder-language`; the typed domain model and its validated edits live in `crates/donder-model`; project parsing, import/source ownership, diagnostics, and serialization live in `crates/donder-project-io`; host-side preparation lives in `crates/donder-elaboration`; portable prepared-sequence evaluation lives in `crates/donder-runtime`. `crates/donder-preview` holds Preview playback and the wgpu renderer, `crates/donder-output` the E1.31 and Art-Net transports, `crates/donder-language-server` the transport-independent language server behind every text editor, and `crates/donder-cli` the local project CLI, including `donder lsp`. `crates/donder-test-support` holds the playback workloads shared by runtime and elaboration tests and benchmarks. The desktop service and UI state live under `apps/desktop/src`; ESP32 firmware lives in `firmware/esp32`. `examples/starter` is the maintained example project. `examples/stanford_room` is a full show used as the representative performance workload. The example projects' `AGENTS.md` files guide an assistant helping a show's author write the show in the Donder language, not work on this codebase; the starter's ships with every new project, so keep them about authoring. Keep synthetic and invalid fixtures beside their owning tests or create them in temporary test directories; do not add scratch projects under `examples/`.

Every document is written in the Donder project language (`docs/project_language.md`): data documents are `*.data.donder` (root `project.data.donder`) and scripts are `*.donder`. The typed `DonderProject` is authoritative after loading. `SourceProject` records document ownership, imports, script source text, and referenced assets. Saving prints data documents directly from typed state with the canonical printer; do not add a synchronization or typed-to-text mutation phase.
Data documents and editor states correspond exactly: every field is explicit and canonical, data documents have no comments (objects have a `description` field), and every name and description the text holds must be shown and editable in the GUI. A save of a loaded project rewrites no byte of a canonical document. Keep that correspondence when adding a field: add it to the document types in `crates/donder-project-io/src/document/types.rs`, the encoder and resolver, and the GUI together. Do not add per-token provenance or comment preservation.
The language's syntax tree, parser, printer, literal rules and schema traits live in `donder_language::data`; the `#[derive(Data)]` macro is `crates/donder-data-derive`. Project/source metadata and ownership live in `crates/donder-project-io/src/source.rs`. Canonical import declarations, aliases, and symbolic references belong to `donder-language/src/imports.rs`; project configuration and safe document paths are validated in `donder-project-io/src/project_config.rs`. Project import expansion, scopes, linking, reference resolution, and edit-time visibility belong to `crates/donder-project-io/src/imports.rs`. Document types, decoding, and encoding live in `document/`; resolution of declarations into typed state in `loader/`. Project loading and checking live in `project_loading.rs`; diagnostics in `diagnostics.rs`; project save/export and sequence insertion in `project_edit.rs`. Keep `lib.rs` as the public facade. Every crate's public API is listed in its `lib.rs` (or, for `donder-language`, in the `data`, `compiler` and `analysis` namespace roots); do not add glob re-exports or reach into a crate's modules from outside. `donder-runtime-types`, `donder-language` and `donder-model` enable the `unreachable_pub` and `unnameable_types` lints, so an item is either exported or crate-private.

Desktop state orchestration is split by workflow under `apps/desktop/src/desktop_state`. Shared typed GUI behavior is split into projection, editing, selection, and model conversion under `crates/donder-editor/src/gui`; shared DTOs live in `crates/donder-sequence-api`. The desktop GUI and DTO modules reexport those crates, and `crates/donder-browser` hosts the same editor in a WASM session for the website. Keep new behavior with the owning workflow instead of growing the module roots.
Mutual Donder document imports are valid. The loader indexes a document's local objects before following imports; do not reject an in-progress document as a cycle error.
Keep import identity and path remapping in project loading and editing, never in per-frame evaluation. Imports are `import alias from <path>, <path>;` with the shared identifier alias policy.

## Testing Guidelines

`pnpm check` includes `pnpm storage:test`, which executes the firmware-owned
device-storage tests on the host from the repository root. Keep this coverage
when changing workspace boundaries. Configure `DONDER_HOST_LIBCLANG_PATH` for
the host gate; `storage:test` passes it to Cargo as `LIBCLANG_PATH` and uses
`target/storage-host` for build artifacts.
`pnpm firmware:build` and `pnpm firmware:cargo` select ESP tools in child processes
using `DONDER_ESP_LIBCLANG_PATH` and `DONDER_ESP_TOOLCHAIN_BIN`.

Rust integration tests live under `crates/*/tests`, and desktop service tests may live beside the service modules. Do not add or modify tests unless specifically requested.
When tests are requested for project analysis, document edits, diagnostics, or model behavior, prefer fixtures from `examples/starter` for realistic project flows and use temporary test directories for invalid or synthetic Donder documents.

## Benchmark Guidelines

Prioritize representative playback performance, especially ESP32 frame times, missed deadlines, and memory use, over isolated microbenchmark percentages. Microbenchmarks are diagnostic tools, not individual acceptance gates. Investigate reproducible regressions with meaningful absolute cost, poor scaling, or a connection to a measured bottleneck; use controlled reruns to distinguish signal from noise. Do not repeatedly chase or justify tiny isolated regressions when representative workloads improve. Briefly record the tradeoff or uncertainty and move on; do not add complexity merely to recover a microbenchmark score. Simplifying the hot path may warrant an intermediate slowdown, but verify the eventual end-to-end result.

Prepared playback and real-project render benchmarks use Criterion only. Use `pnpm bench:effect-vm:quick` for a fast smoke pass, `pnpm bench:effect-vm:save` before optimization work, `pnpm bench:effect-vm:compare` after optimization work, and `pnpm bench:effect-vm` for the full benchmark set. Focused runs are `cargo bench -p donder-runtime --bench prepared_playback_bench -- prepared_effect_suite_4x512_pixels` and `cargo bench -p donder-elaboration --bench render_bench -- controller_output_dense_60_frames`.

Do not reintroduce custom benchmark CLIs, JSON reporters, legacy aliases, or old render bench flags such as `--project`, `--frames`, `--iterations`, `--warmup`, or `--render-only`. Criterion output lives under `target/criterion` and must not be committed. Timing changes are advisory; checksum and active-effect-count assertion changes are behavior changes unless intentional.

## Agent-Specific Instructions

### Development version policy

Donder is pre-release software with no users or compatibility obligations. Treat
the Donder product version and the current project/serialization formats
as one moving development line, currently `0.1` / `0.1.0` where a full
semantic version is required. Do not add migrations, compatibility layers,
legacy aliases, version ranges, or support for intermediate versions that were
created during local development. When an authored format or internal wire
format changes, update the current implementation, fixtures, and documentation
in place; old local data may be discarded or regenerated.

Keep safeguards that detect malformed data, corruption, impossible values, or
an actually incompatible current format. A numeric protocol/schema version may
remain when it is needed to reject a mismatched payload, but it is a current
format marker rather than a promise to support historical versions. Dependency
versions in Cargo and pnpm manifests are third-party constraints and are not
part of this policy. Revisit this policy only when Donder is preparing for real
external users or a deliberate compatibility commitment.

### Dependency maintenance policy

Dependency freshness is checked locally; do not depend on Dependabot or other
external update PRs. Before adding a dependency, check its current upstream
release and use the latest compatible release for the target platform and
feature set. Do not knowingly add an older release unless an explicit upstream
compatibility constraint, local patch, or measured regression requires it;
record that reason beside the dependency.

When reviewing or preparing dependency updates, inspect both Cargo workspaces
and the pnpm workspace. The ESP32 workspace under `firmware/esp32` has its own
manifest and lockfile and must be checked separately from the desktop
workspace. Use read-only checks such as `pnpm outdated`, `pnpm audit`,
`cargo outdated --workspace`, `cargo deny check advisories -W unmaintained`,
and `cargo tree -i <crate>`. Use `--manifest-path
firmware/esp32/Cargo.toml` for firmware `cargo outdated` and `cargo tree`
checks; run `cargo deny check advisories -W unmaintained` from
`firmware/esp32` because cargo-deny does not accept `--manifest-path`.
Treating the unmaintained lint as a warning keeps vulnerability advisories
fatal while still printing inherited packages that have no safe upgrade. Check direct and
transitive dependencies, including SDK and HAL packages. Do not update
anything automatically, and do not update a lockfile or manifest as part of an
inspection. Separate available upgrades from security advisories,
unmaintained transitive packages, upstream Git patches, and intentionally
pinned hardware SDK revisions. Summarize those findings before making changes.

Never reinvent a pattern or solve a problem that has already been solved. Use dependencies (after asking the user) to solve problems rather than reinventing the wheel.
Avoid using strings in internal logic. Prefer enums or other structured data.
All static color literals must be defined in `apps/desktop/frontend/src/styles.css` as CSS custom properties. TypeScript, JSX, Rust, and tests must reference CSS-backed tokens or receive data-driven colors; do not define palette values elsewhere.
Always use `apps/desktop/frontend/src/styles.css` as the styling source of truth and `apps/desktop/frontend/src/theme.ts` as the runtime bridge for CSS-backed values. Never hardcode styling values in TypeScript or JSX. Reuse existing CSS classes and tokens whenever they fit; when they do not, add a clearly named semantic style or token to `styles.css` and expose it through `theme.ts` when runtime code needs it.
All static frontend styling values—including typography, spacing, dimensions, shape, elevation, layering, motion, opacity, form geometry, icon sizes, scrollbar geometry, visualization metrics, responsive breakpoints, and accessibility geometry—must be defined in `apps/desktop/frontend/src/styles.css`. TypeScript and JSX may only use CSS-backed values or genuinely runtime/data-dependent values such as measured geometry, coordinates, and user/project colors.
`apps/desktop/frontend/src/generated/bindings.ts` and `apps/desktop/gen/schemas/` are committed generated API artifacts. Regenerate bindings with `pnpm generate:bindings` and schemas through the Tauri tooling; never hand-edit either.
Avoid unrelated edits to lockfiles, IDE files, or generated assets.
Keep temporary Donder scripts, profiling captures, and build artifacts under an ignored `target/` directory inside this repository, not in the user's home directory. Do not commit raw logs, measurement captures, profiler output, screenshots, or step-by-step implementation journals; summarize a durable result, with its date and conditions, in `docs/performance.md`. Shared installed toolchains and package caches may remain in their standard locations.
Documentation describes current behavior and durable contracts. Consolidate superseded investigation notes into the current architecture or performance reference, then remove the journal; links are not a reason to retain stale documents.
Check both Rust and desktop manifests before assuming a command or dependency belongs at the workspace root. `firmware/esp32/crates/donder-device-storage` is firmware-only and belongs to the ESP32 workspace. Keep embedded-only dependencies out of the root host workspace, and validate them with the firmware manifest and toolchain.
Do not add compatibility layers, shims, fallbacks, or allow for legacy code when adding features or refactoring.
Do not add fallbacks when something doesn't work. This hides errors and makes debugging harder.
The goal is fast development, not support. Minimize clutter and favor having a single way of doing things. SSOT is your friend.
Put language semantics in `donder-language`, domain rules in `donder-model`, preparation in `donder-elaboration`, and portable frame-evaluation semantics in `donder-runtime`; desktop projection code must not reimplement any of them.
An item belongs in `donder-runtime-types` only if the runtime consumes it and an upstream crate produces it; playback logic stays in `donder-runtime` and authoring logic stays upstream. Firmware places `sampling` and `values` functions in IRAM by module name (`firmware/esp32/rwtext_hook.x`), so keep those module names.
`DesktopState` owns GUI edit transactionality and history. Loaded and historical project snapshots are immutable `Arc<ProjectSession>` values; a GUI edit makes one deep candidate clone, then shares the accepted snapshot with state, history, save, render-refresh, and clip-raster work. GUI mutation helpers edit the candidate session they receive; do not add another whole-session transactional clone inside them.
Desktop background save/render scheduling and GUI history storage live in `state_tasks/`; use its single latest-request scheduler rather than adding another channel scheduler. Read-only fixture/layout geometry projection lives under `preview/geometry.rs`, outside GUI mutation dispatch.
Sequence waveform decoding, cache management, and drawing live in `apps/desktop/frontend/src/ui/gui/sequence/sequenceWaveform.tsx`; keep audio processing out of `SequenceCanvas.tsx`, and pass palette values into the waveform renderer instead of duplicating colors.
Source object kind conversion to desktop `ObjectKind` belongs in the DTO boundary. Do not duplicate that mapping in state or GUI modules.
GUI edits must mutate typed domain state only. Do not construct, inspect, or mutate document text directly from GUI edit code.
GUI edits must not run project checks or reload from text after mutation. Persistence belongs to the IO save path.
Do not use git or commands associated with it unless the user specifically requests it.
Do not use .env files to store information.
Do not jump to editing if the conversation is about diagnosing an issue or discussing architecture/design decisions.
Do not start or leave a frontend dev server running when finishing work. The user needs `pnpm tauri dev` to own the frontend port.
When planning, don't hesitate to ask the user relevant questions.
When presenting a plan for approval from the user, list files that will be affected by the plan.
Run `cargo fmt` and then `pnpm check` for linting, tests, and other checks after implementing a plan. Fix any regressions. Running these checks is not necessary after updating docs, example projects, or other things unaffected by tests or checks.
