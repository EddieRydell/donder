# Authoring architecture

Dawn authors a typed project rather than editing YAML as an application model.
`dawn-project-io` loads source documents into `DawnProject` and records document
ownership, imports, original non-YAML source, and asset references in
`SourceProject`. GUI edits mutate a single candidate `ProjectSession`; accepted
state is then shared with history, persistence, rendering, and waveform work.

## Owning layers

- `dawn-language` owns domain types, the effect/operator DSL, and semantic
  validation.
- `dawn-project-io` owns source documents, imports, linking, diagnostics,
  serialization, and package/project loading.
  Its public facade reexports package loading/checking from `package_loading.rs`,
  release planning/validation from `package_artifact.rs`, project edits and
  save/export from `project_edit.rs`, and diagnostics/source indexing from
  `diagnostics.rs`. YAML serialization stays in `serialization/`.
- `dawn-elaboration` expands generators, resolves targets, and prepares the
  portable runtime representation.
- `dawn-runtime` evaluates prepared sequences. It does not resolve source names,
  imports, layouts, or device selection per frame.
- `apps/desktop/src/desktop_state` owns application workflows and background
  scheduling. `apps/desktop/src/gui` owns typed projection, edits, selection, and
  DTO conversion. The frontend renders those typed contracts.

The typed project is authoritative after load. Saving derives canonical YAML
from typed state. There is no typed-to-YAML synchronization phase and no CST or
per-scalar provenance model. See [Sequence as Code](sequence_as_code.md) for the
preservation contract.

## Main authoring workflows

Layout authoring creates pixel-only fixture definitions and places reusable
instances in layout groups. Instance order and definition pixel order determine
the logical pixel domain. Patch authoring routes instance pixel ranges directly
to controller ports with explicit RGB/RGBW encoding and channel order. Sequence
authoring targets layout instances or groups and edits typed effects, operators,
parameters, automation, and timing.

Definitions shared by multiple instances remain one object. Imported dependency
objects are read-only; an editable copy is created explicitly. Mutual document
imports are valid because the loader indexes local objects before following
imports. DSL generators declare their own imports and never inherit the caller's
YAML scope.

Each GUI edit deep-clones the session once, applies the mutation to that
candidate, validates the edit contract, and publishes the accepted immutable
snapshot. A rejected edit leaves the prior snapshot untouched. Save and render
refresh are scheduled from the accepted revision; they do not introduce another
editable copy or reload YAML to validate a GUI mutation.

## Playback boundary

Elaboration flattens fixture definitions, expands generators, resolves symbolic
references, and assigns numeric child slots. Prepared events carry only those
slots. Runtime evaluation uses flat buffers and direct output routes; it must not
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
