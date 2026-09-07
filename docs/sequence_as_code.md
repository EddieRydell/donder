# Sequence-as-Code Contract

A Dawn sequence is declarative YAML that names a duration, frame rate, layers,
effect instances, a composition graph, automation clips, and control clips. It
is loaded into the typed `dawn_language::sequence::Sequence`; YAML is never an
editable runtime model after load.

## Fixed parameter declarations

Effects and operators can declare editable preparation values using
`fixed param int count = 8;`. An ordinary `param` retains its existing type-based
automation eligibility (float, int, bool, enum, and curve). Fixed parameters
cannot receive active automation, including through detached rebinding. The
editor labels them as requiring preparation and keeps their values editable.

Generator timing, target selection, and control flow determining emission must
depend only on fixed parameters and preparation context. Dependency checking
follows locals, arrays, assignments, branches, and loop-carried values. Generator
pixel-context reads are rejected. Linked child argument types and live-to-fixed
arguments are checked even in unused branches.

Preparation expands structural branches and loops into concrete children. Child
parameters retain constants, numeric parent-parameter references, or typed VM
calculations. Fixed locals, including loop indices, are captured at each emission.
Pure live arithmetic, resource selection, arrays, branches, and bounded loops can
compute rendering parameters. The existing VM execution and array limits apply.
Unchanged projects without automation or time dependencies keep constant bindings
and the static playback path.

Retained expressions use their declaring generator's `seconds()` and `progress()`
at the exact requested time. Nested forwarding preserves that clock; each child's
`sample()` uses its own clock. Bindings remain available when children outlive
their parents. Existing automation positioning and endpoint rules apply during
ordinary frames, temporal/spatial queries, and backward seeks. Parameter and
resource workspaces are reserved during preparation, with exact-time caches
shared across pixels. Resource references are forwarded without rebuilding them.

Native MarkPulse fixes beats, offset, decay duration, section width, sections per
mark, and seed. MarkChase fixes beats, offset, and chase duration. Their other
rendering inputs use the same retained bindings. Resource validity checks still
apply, including nonempty MarkChase collections. MarkImpactBurst's gradient
collection is fixed because its emptiness determines whether a child is emitted.

Declaration metadata governs automation even when an instance has no active
automation. Fixed child arguments and assignments cannot receive live values;
structural branches are rejected conservatively, including branches that appear
to emit equivalent children. Imported and unused emissions undergo the same
type, required-argument, and fixed/live checks before expansion. Authored active
automation targeting a fixed parameter is an error. Definition replacement keeps
the explicit detached-binding workflow; detached bindings cannot activate against
a fixed parameter. See [the implementation record](signal_model_work.md) for
verification evidence.

## Operator signal coordinates

An operator samples an immutable input signal by time and pixel:

- `source.at(seconds)` samples the current pixel.
- `source.at(seconds, pixel)` samples a zero-based pixel in the current fixture
  (color element). For example, `source.at(seconds(), pixel_count() - 1 -
  pixel_index())` mirrors each fixture independently.
- `source.at_global(seconds, pixel)` samples a zero-based pixel in the full
  prepared rig's color-element order, with each element's pixels contiguous.
  This order is independent of selected controller ports and output packing.

Time arguments are seconds (float); pixel arguments are integers. Negative or
out-of-range pixels return black, as do times outside the sequence. Non-finite or
unrepresentable times are errors. Local coordinates never wrap across fixtures.
These queries can combine spatial and temporal transformations and can sample
other operators. Repeated queries are immutable; caches include every coordinate
that affects their result.

Controller preparation retains upstream pixels reachable through spatial reads,
even when they are not patched to that controller. Local reads require the whole
selected fixture; unrestricted global reads require the full rig color domain.
This can increase device memory requirements; output packing still includes only
the selected ports. Authored effect targets and per-fixture/whole-target scope
remain separate, fixed settings, not automatable effect parameters.

## Preservation contract

Semantic, source-aware serialization is the intended level of preservation.
Canonical YAML is acceptable, including for projects primarily authored by LLMs.
Lossless text editing is not a goal for GUI/project saves.

For a valid project edited through supported workflows, saving and reloading
must preserve:

- Typed authoring values, IDs, references, and list order wherever it carries
  meaning (clips, layers, curve points, graph connections, and similar data).
- Each independently named object's owning module, document, and object name;
  objects remain in their owning files rather than being flattened into one file.
- Import declarations, aliases, and resolved targets, except when an explicit
  structural edit changes them. Cross-document references must remain resolvable.
- Referenced asset identity and project-relative path policy.
- Effect and operator DSL source, retained as text rather than regenerated from
  compiled programs. Typed YAML serialization does not decompile the DSL.

We intentionally do **not** promise YAML comments, whitespace, indentation,
quoting, mapping key order, original numeric/duration spelling, anchor/alias
syntax, or the distinction between an omitted default and an explicit default.
Equivalent source spellings may become one canonical representation. This is
not permission to reorder semantic lists or discard meaningful authoring data.
Arbitrary extra YAML metadata is not an extensibility/preservation API.

Provenance belongs to independently stored objects and documents. A nested
clip or scalar inherits its containing object's file; it does not need its own
source span or editable concrete-syntax node. Loading diagnostics can use spans
without making those spans the persistence model. This metadata stays outside
portable frame evaluation; it does not require a VM/runtime redesign.

Do not introduce a lossless YAML/CST editor, bidirectional AST synchronization,
or per-scalar provenance merely to preserve presentation. Revisit this decision
only when a concrete authoring requirement cannot be met by typed serialization
plus document ownership/import metadata.

### Current limitations

- Structural mutations must maintain typed state **and** the source object
  inventory/import graph through the owning IO workflows. Saving rejects missing
  typed objects and typed objects without source inventory before writing any
  document; it never restores original YAML or silently omits a new object.
  All objects in loaded documents are resolved into typed state, including unused
  objects. Files that were never loaded are not part of that inventory.
- Missing imports for typed cross-document references cause serialization errors;
  serialization does not invent an alias or flatten the referenced definition.
  Reference-changing workflows must arrange imports before saving.
- Typed effect-parameter values reject unknown keys according to their variant,
  including recursive array items and curve/gradient array shorthands. Diagnostics
  identify the unexpected field and its value location. This is not a promise to
  retain arbitrary metadata or a claim that every other schema mapping has been
  exhaustively audited for unknown-key rejection.
- Saving retained DSL text does not project edits to compiled effect/operator
  definitions back into source. Generated child effects are derived preparation
  output, not independently editable source objects. Editing generated output
  back into arbitrary generator code is outside this contract.
- A generator's unqualified `timeline.emit Child` resolves `Child` in the
  generator definition's document, not in the calling YAML document's import
  group. Cross-file children require an explicit effect-document import (below).
  Imports do not re-export imported names, and operator documents do not support
  imports: operators currently have no corresponding cross-file call construct.
- Project save writes project-owned documents, not dependency documents. This
  contract is not a dependency vendoring or cross-package editing mechanism.
- Semantic round-tripping is not byte-identical round-tripping. The separate
  source-text write API writes supplied text exactly; that does not imply that a
  later typed project save preserves its presentation.

### Verification coverage

`crates/dawn-project-io/tests/semantic_preservation.rs` exercises a typed edit of
`examples/starter`, full typed-project equality after save/reload, meaningful
list order, document inventories/ownership, import edges, asset references,
retained DSL text, and canonical serialization stability. It also verifies
unknown-key rejection, unused-object preservation, missing-import diagnostics,
and refusal to save inconsistent inventories without touching files.
`crates/dawn-project-io/tests/roundtrip.rs` additionally checks same-named definitions in different files
through save/reload and insertion of a sequence in a new nested file.
`crates/dawn-elaboration/tests/generator_source_scope.rs` checks cross-document
and local generator children, mutual imports, rejection of caller-scope lookup,
and actual starter generator emission with nonempty marks/gradients.
`crates/dawn-project-io/tests/path_refactor.rs` covers import-path moves and
dependency-export identity through save/reload and dependency document moves.

These are focused IO/preparation checks, not an exhaustive GUI action matrix,
proof for every schema field, or a rendered-output equivalence benchmark.

## Generator imports

Effect imports precede declarations and use the existing module/package import
resolver. Local paths are **module-root relative**, just like YAML imports, not
relative to the effect file. Local DSL imports use the shared non-empty
document-list form:

This is a breaking authoring correction within `languageVersion: "0.1"`.
Single-string local DSL imports are rejected; there is no compatibility parser.

```text
import bursts from ["effects/impact-burst.effect.dawn"];

effect Hits {
  param gradient palette;
  param curve intensity;

  void generate() {
    timeline.emit bursts.ImpactBurst {
      start: 0.0,
      duration: 0.45,
      target: target,
      gradient: palette,
      intensity: intensity
    };
  }
}
```

For a declared package dependency, use `import bursts from library.effects;`,
where `library` is the manifest dependency alias and `effects` is its export
group. Package names may contain hyphens; local aliases use letters, digits,
and underscores, with a non-digit first character; keywords and `builtins` are
reserved. The export group can contain several documents, with unique object
names. Imports are explicit, not transitive. Duplicate
aliases, duplicate target documents, unsafe/missing paths, unresolved children,
and child references to non-effect objects are errors.

Local declarations are indexed before following imports, so mutual document
imports are valid. Every compiled emitted child reference is checked during
loading, even if that emission would never execute. Recursive *generation* is
still subject to the preparation depth and generated-effect budgets.

Language compilation retains symbolic emitted references and diagnostic spans
separately from portable bytecode. Project IO links every emitted child into an
ordered target table, including local and built-in children. The VM returns only
a typed numeric slot; elaboration indexes the linked table directly. Diagnostic
spans do not affect semantic equality or compilation/cache signatures.

Import declarations and aliases have one language-owned representation. Project
IO builds scopes after all reachable local inventories are available and uses
the same lookup for YAML references and emitted children. Imports expose only
their targets' own objects. Same-document GUI references need no import; other
local selections reuse an import or create deterministic aliases such as
`effects_2`. Dependency selections require an explicit export import.

Prepared playback bytecode does not retain generator import
tables, and no per-frame path or name resolution is introduced. Structural path
edits update explicit DSL import-path tokens and resolved identities; ordinary
saves retain the DSL text. This narrow source edit is not general AST-to-DSL
serialization.

## Validity

The canonical validator is `dawn_language::validation::validate_sequence`.
Project loading and checking, accepted GUI edits, and runtime preparation all
use that validator. A sequence must satisfy these rules:

- `duration` is finite, non-negative source input and is positive once loaded.
- `frame_rate` is greater than zero, and `duration * frame_rate` cannot exceed
  250,000 prepared frames.
- Layer, effect, mark-collection, automation-clip, and control-clip IDs are
  unique. Timed objects fit within the sequence duration.
- Effects reference an existing layer, a compatible color target, a defined
  effect, and only its declared parameters. Required parameters must be
  supplied; unknown parameters are invalid.
- The composition graph has one output and valid acyclic, typed connections.
  Its layer nodes reference distinct sequence layers.
- Active automation targets exist, have a compatible mapping, and are unique
  even relative to detached bindings. Detached bindings preserve a historical
  unresolved target only after an explicit detachment reason.
- Control clips are valid, in range, and non-overlapping for the same target.

## Curves

Curves are normalized, piecewise-linear values. They must contain at least one
point; each point’s position and value must be finite; positions are in
`[0, 1]` and strictly increasing. All sequence automation, native effects, and
DSL curve reads use `dawn_language::sampling::sample_curve`.

## Source diagnostics

Present optional values must have their declared shape. For example,
`automation_clips: wrong` is an error, not an empty clip list. The sequence
object's fields are closed: unknown keys there are errors. This does not yet
hold for every nested mapping; see the preservation limitations above.
Duration parsing is fallible and never invokes panicking duration
constructors.

The effect and operator DSL reports parse/type errors rather than changing a
bad literal to zero. Integer division produces a float; integer overflow and
remainder by zero are VM errors. Required DSL parameters cannot receive an
implicit type default.

## Runtime budgets

The renderer limits a prepared sequence to 250,000 frames, generated effects
to 100,000 per preparation, and custom-operator Signal sampling to 4,096 unique
times per operator render. The DSL VM limits each invocation to 10,000 loop
iterations. Exceeding a budget returns an error; Dawn does not silently clamp
or skip work.

## Authoring

- Use `.sequence.dawn` YAML for sequence data.
- Use `.effect.dawn` for custom effects and `.operator.dawn` for custom graph
  operators; both receive DSL highlighting in the desktop editor.
- Start from `examples/starter` for valid curve, effect, operator, and graph
  examples.
