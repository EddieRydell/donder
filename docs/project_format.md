# Project format

A Donder project is a folder of `.donder` documents. YAML documents hold
fixtures, layouts, patches, setups, controllers, curves, gradients and
sequences; `.effect.donder` and `.operator.donder` documents hold
[effect language](effect_language.md) source. Project IO loads them into the
typed `DonderProject`, which is authoritative from then on.

## Root document and imports

Every project has a fixed `project.donder` at its root. Its reserved `workspace`
block stores `format_version: 1` and a stable, non-nil `project_id` UUID, which
establishes document and object identity before imports resolve. The root
document contains imports, exactly one project object, and optionally other
objects. It cannot be moved, renamed or deleted; there is no other manifest,
lockfile or asset inventory.

```yaml
imports:
- from:
    documents:
    - operators/standard.operator.donder
  as: operators
```

- Import paths are relative to the project root and cannot leave it.
- Aliases use letters, digits and underscores and cannot start with a digit;
  keywords and `builtins` are reserved. One import may list several documents
  whose object names are unique.
- An import exposes its documents' own objects, not what those documents import.
  Each consuming document declares the imports it uses.
- Mutual imports are valid: the loader indexes a document's own objects before
  following its imports.
- Effect and operator documents contain declarations only; they have no imports
  or cross-document calls.
- When the GUI references an object in another document, it reuses an import or
  creates a deterministic alias such as `effects_2`.

Prepared playback never sees paths, aliases or symbolic names.

## Owned and reusable values

A value is either owned by its parent (`ValueSource::Inline`) or a reference to
a separately named reusable object (`ValueSource::Reference`), whether that
object is in the same file or another. This applies to project setups and
sequences, a setup's layout, patch and controllers, fixture geometry, curves and
gradients. Effect and operator code is always named DSL source. Instance
parameters and nested clips, layers and groups are always owned.

Owned collection members (controllers, sequences) carry a numeric `id` unique
within their owner, which survives reordering. Patch routes and effect targets
can address an owned layout or controller by path without creating a symbol:

```yaml
layout: {owner: show, path: [setup, layout]}
controller: {owner: show, path: [setup, {type: controller, id: 3}]}
```

`owner` uses the normal import scope. Paths may only step through owned slots,
never through references.

The editor's source actions move values between the two forms:
- **Make reusable** promotes an owned value to a named source and rebases
  references to its descendants.
- **Make independent** copies a linked value into its parent, leaving the
  source unchanged.
- **Use existing source** links a slot to a named object.

Linked curves and gradients follow their source; making one independent keeps
its current value in the parameter. New sequences, fixtures and other values
are owned by default; their creation dialogs offer a reusable source in this
file or a new file under Advanced settings.

Routing follows these changes. Making a layout or controller independent also
makes its patch independent when routes must change. For the active setup,
sequences targeting a copied layout become independent before their targets
change. Imports are maintained from typed references, and undo/redo includes
source registration and imports.

Audio is a project-relative path; copying a sequence keeps the path and does not
copy audio bytes. **Create Standalone Project Copy** (and `donder copy`) writes
a separate project containing the loaded sources and referenced audio.

New projects start as a single `project.donder` with an owned setup, an empty
layout and patch, and one owned sequence. The root document imports the bundled
standard effect and operator libraries, so the initial sequence can use them;
separate sequence documents need their own imports.

## Sequences

A sequence has a duration, a frame rate, optional audio, and five kinds of
content:

- **Mark collections:** named lists of beats or cues.
- **Layers:** a name, a color and an enabled flag.
- **Effects:** a clip on a layer with a `target` (a layout instance or group),
  a start and duration, and parameter values.
- **The composition graph:** layer, operator and output nodes with typed edges.
  Layers produce signals, operators combine and transform them, and the single
  output node is what plays. Nodes not connected to the output may remain while
  you edit, and are not prepared. An operator on the output path must have every
  input connected.
- **Automation clips:** a curve placed on a timeline row (`row_target`), with
  bindings that map the curve onto parameters of effects (`effect_param`) or
  graph operators (`composition_node_param`).

Every layout fixture or group has one effect row and one automation row. A
clip's row is only placement: moving it between rows does not change its
bindings. One clip can bind several parameters, each with its own mapping:
a float, int or curve range, a bool (on at 0.5), or a list of enum values.
When the GUI deletes a bound effect or operator, or replaces its definition so a
parameter no longer fits, the binding moves to `detached_bindings` with that
reason, so it can be reattached rather than silently lost.

In the editor:
- Copy and paste of effects and automation together keeps relative timing and
  target offsets, allocates new IDs, and remaps bindings to the copied effects.
  Bindings to objects outside the copied selection are dropped; a lone clip
  pastes unbound.
- A paste that does not fit is rejected as a whole. Moving, resizing, cutting or
  pasting a selection is one history entry.
- In the graph, operator parameters and automation controls live inside each
  node, and layer controls inside layer nodes. Right-click to add or delete;
  deleting a layer asks where its effects should move. Default and Output cannot
  be deleted.
- Row heights, node sizes, graph pan and zoom and guides are workspace
  preferences, not project data, and are outside undo history.

## Validity

`donder_language::validation::validate_sequence` is the single sequence
validator. Project loading and checked GUI edits run it before accepting state;
preparation does not repeat it.

- Duration is positive. Frame rate is positive, and duration × frame rate is at
  most 250,000 frames. All times fit the portable 32-bit microsecond clock, and
  positive durations cannot round to zero.
- Layer, effect, mark-collection and automation-clip IDs are unique, and timed
  objects fit within the sequence.
- Effects reference an existing layer, a compatible target and a defined effect,
  supply every required parameter and no unknown ones.
- The graph has one output, typed acyclic connections, and layer nodes that
  reference distinct layers. Operator definitions, parameters, port types and
  cardinality are checked even on disconnected branches.
- Active automation bindings target existing, compatible, non-fixed parameters,
  each at most once (including relative to detached bindings).

Fixture rules are in [fixture authoring](fixture_authoring.md).

## Strict parsing

Every object and nested mapping is closed: an unknown key, including a
misspelled optional field or a field of another variant, is an error with its
source location. A present optional value must have its declared shape
(`automation_clips: wrong` is an error, not an empty list). Object names and
parameter names are deliberate dictionary keys; their values are parsed
strictly. Durations parse fallibly. Curve points and gradient stops use strict
Serde schemas, with field paths attached through `serde_path_to_error`.

## Saving and preservation

Saving serializes typed state to canonical YAML. It never edits the original
text, and there is no CST or per-scalar provenance. Each file is written through
`donder_project_io::atomic_write`, which replaces it with a complete synced
temporary file. That keeps any one file from being truncated, but a multi-file
save is not a crash-atomic transaction.

A save and reload preserves:

- typed values, IDs, references, and every list order that carries meaning
  (clips, layers, curve points, graph connections);
- each named object's owning document and name, so objects stay in their files;
- import declarations, aliases and resolved targets, unless an edit changes them;
- asset paths;
- effect and operator source text, which is retained, never regenerated.

Comments, whitespace, quoting, key order, numeric spelling, anchors and the
difference between an omitted and an explicit default are not preserved.

Saving refuses, before writing anything, a typed object with no source document,
a missing source object, or a cross-document reference with no import; it never
invents an alias or flattens a definition. All objects in loaded documents are
typed, including unused ones. Unreferenced files are left alone. The source-text
write API writes supplied text exactly, but a later typed save may reformat it.

Change this contract only for a concrete authoring requirement that typed
serialization plus document ownership cannot meet.

`crates/donder-project-io/tests/semantic_preservation.rs`, `roundtrip.rs` and
`path_refactor.rs` cover a typed edit of the starter with full save/reload
equality, list order, ownership, imports, assets, retained DSL text, unknown-key
rejection, refusal of inconsistent saves, same-named objects in different files,
and import-path moves.
