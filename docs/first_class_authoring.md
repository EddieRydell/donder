# Authoring architecture

Donder authors a typed project rather than editing YAML as an application model.
`donder-project-io` loads source documents into `DonderProject` and records document
ownership, imports, original non-YAML source, and asset references in
`SourceProject`. GUI edits mutate a single candidate `ProjectSession`; accepted
state is then shared with history, persistence, rendering, and waveform work.

## Owning layers

- `donder-language` owns domain types, the effect/operator DSL, and semantic
  validation.
- `donder-project-io` owns source documents, imports, linking, diagnostics,
  serialization, and local project loading.
  Its public facade reexports project loading/checking from `project_loading.rs`,
  project configuration from `project_config.rs`, project edits and
  save/export from `project_edit.rs`, and diagnostics/source indexing from
  `diagnostics.rs`. YAML serialization stays in `serialization/`.
- `donder-elaboration` expands generators, resolves targets, and prepares the
  portable runtime representation.
- `donder-runtime` evaluates prepared sequences. It does not resolve source names,
  imports, layouts, or device selection per frame.
- `apps/desktop/src/desktop_state` owns application workflows and background
  scheduling. `apps/desktop/src/gui` owns typed projection, edits, selection, and
  DTO conversion. The frontend renders those typed contracts.

The typed project is authoritative after load. Saving derives canonical YAML
from typed state. There is no typed-to-YAML synchronization phase and no CST or
per-scalar provenance model. See [Sequence as Code](sequence_as_code.md) for the
preservation contract.

## Main authoring workflows

Layout authoring places fixtures and groups. Fixture geometry contains ordered
pixels, lines, polylines, arcs, and grids. Each shape controls its pixel count
and order. Patch authoring routes instance pixel ranges to controller ports with
explicit RGB/RGBW encoding and channel order. Sequence authoring targets layout
instances or groups and edits effects, operators, parameters, automation, and timing.

Owned values live inside their parent. Reusable values have a named source in
this file or another file. The same source actions apply to project setup and
sequences, setup layout/patch/controllers, and layout fixture geometry:
**Make reusable** moves an owned value to a named source and leaves a link;
**Make independent** copies a linked value into its parent and keeps the source
unchanged. The reusable-source dialog places its storage choice under Advanced
settings below Name. Inline values do not acquire synthetic source keys.

Routing follows independent layout/controller copies. A shared patch becomes
owned before changing its routes; active sequences become owned before their
layout targets change. Undo and redo include source registration and imports.
Imported sources are ordinary editable files inside the project.
A standalone project copy creates and opens a separate project with local copies
of imported sources and referenced audio.

Linked curves and gradients are references to shared definitions, not local
parameter values. **Open source** navigates to the definition; changing a writable
source affects parameters linked to it. **Make independent** retains the
current curve or gradient directly in this parameter, leaving the source
unchanged and no longer following its changes. The dropdown, preview editing,
and flip actions explain and confirm unlinking through the same dialog.

Mutual document
imports are valid because the loader indexes local objects before following
imports. DSL generators declare their own imports and never inherit the caller's
YAML scope.

Each GUI edit deep-clones the session once, applies the mutation to that
candidate, validates the edit contract, and publishes the accepted immutable
snapshot. A rejected edit leaves the prior snapshot untouched. Save and render
refresh are scheduled from the accepted revision; they do not introduce another
mutable session clone or reload YAML to validate a GUI mutation.

## Playback boundary

Compilation assigns numeric slots to generator emission sites, and project loading
links their symbolic child references in the defining document's import scope.
Elaboration flattens fixture definitions, expands generators, and resolves each
emitted slot through that linked table. It prepares concrete sample effects and
retained parameter calculations; playback has no generator emission instructions.
Runtime evaluation uses flat buffers and direct output routes; it must not
reconstruct source identity or repeat import, target, or fixture traversal.

Output selection is also an elaboration concern. A controller fragment retains
the data required to evaluate and pack its selected ports while preserving the
authored global and per-instance signal coordinates. See
[Output selection](output_selection.md).

## User-facing path

The maintained example is `examples/starter`, and the primary walkthrough is
[Create your first LED show](first_show.md). Fixture details are in
[Fixture authoring](fixture_authoring.md); persistence behavior is in
[File persistence](file_persistence.md). Invalid and synthetic projects belong
beside the focused tests or in temporary directories, not as additional root
examples.

Documentation describes current behavior and durable contracts. Completion
logs, dated implementation plans, debugging transcripts, and benchmark diaries
are intentionally not retained as product documentation. Reproducible benchmark
commands and the small accepted hardware evidence set are documented separately
in [Performance and hardware evidence](performance.md).
