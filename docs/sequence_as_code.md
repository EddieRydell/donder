# Sequence-as-Code Contract

A Donder sequence is declarative YAML that names a duration, frame rate, layers,
effect instances, a composition graph, and parameter automation clips. It
is loaded into the typed `donder_language::sequence::Sequence`; YAML is never an
editable runtime model after load.

## Timeline rows and clipboard

Each fixture or group in the active layout has one effect row and one automation
row. Row identity is the target identity plus row kind; flattened display indices
are temporary geometry only. Effects use `target`; automation clips use
`row_target` with the same `{ layout, fixture }` reference syntax. Automation
placement is independent of its parameter bindings. Moving a clip between rows
does not retarget its bindings.

Empty automation rows are hidden and become available during clip dragging.
Multiple automation clips share a row; overlapping clips use visual slots within
that row. Both row heights are retained by fixture identity in the sequence's
viewport preferences, including while a row is hidden. Row heights are view
preferences, outside project undo history.

Timeline selection can include effects and automation together. Copy/paste keeps
relative timing and target offsets, allocates new clip IDs, and remaps automation
bindings to effects included in the copied selection. Bindings to objects outside
a copied selection are not copied; copying an automation clip alone produces an
unbound envelope. Cutting and pasting within the same sequence retains external
bindings that remain available. A paste that cannot fit within the target list or
sequence duration is rejected atomically. Moving, resizing, cutting, and pasting
mixed selections each create one history entry. Undo/redo clears transient clip
selection and drag state while restoring the authored snapshot.

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

The editable `effects/standard.effect.donder` document defines Pulse, Chase,
Spin, Wipe, MarkPulse, MarkChase, and MarkWipe. MarkPulse emits Pulse children for selected
sections, and MarkChase emits Chase children for each mark. Neither mark effect
adds hue shifting or a separate sampler. Their structural parameters are fixed;
the emitted children's rendering parameters use ordinary retained bindings.
Resource validity checks still apply, including nonempty MarkChase collections.
Spin samples `chase_position` as a position on a strand extended by
`revolutions`, then wraps that position onto the real strand with modulo. Its
pulse shape is sampled by spatial distance behind the head; evaluation does not
loop over revolutions or require a monotonic chase curve.
Wipe projects layout-space pixel positions onto `direction_angle` in degrees:
0 travels toward +X, 90 toward +Y, and 180 toward -X. It traverses the projected
XY bounding rectangle of the selected scope, independent of pixel ordering.
`pulse_width` is the fraction of effect duration occupied by each pixel's pulse;
`wipe_position`, pulse shape, gradient modes, reverse, and extension controls
follow Chase's timing semantics. A scope with no extent along the direction
pulses simultaneously. The angle is fixed for the lifetime of a Wipe.

MarkWipe emits Wipe children, cycling `gradients` and `wipe_positions` like
MarkChase. Its fixed `direction_angle` curve uses turns: 0 is 0 degrees,
0.25 is 90 degrees, and 1 is 360 degrees. It is sampled at each mark's local
time divided by the generator duration, then converted to degrees before
applying `offset_seconds`. Each child's angle is captured during preparation, so overlapping
wipes keep their individual directions. The curve remains editable but cannot
receive live automation.

The sample DSL exposes `pixel_x()`, `pixel_y()`, `target_min_x()`,
`target_min_y()`, `target_max_x()`, and `target_max_y()` in layout meters.
Elaboration uses the shared fixture geometry expansion and transforms; runtime
and clip rasters consume the same spatial contexts. Device fragments preserve
the original scope bounds. Projects without spatial programs omit spatial data
from their prepared runtime payload.

MarkImpactBurst's gradient collection is fixed because its emptiness determines
whether a child is emitted.

Declaration metadata governs automation even when an instance has no active
automation. Fixed child arguments and assignments cannot receive live values;
structural branches are rejected conservatively, including branches that appear
to emit equivalent children. Imported and unused emissions undergo the same
type, required-argument, and fixed/live checks before expansion. Authored active
automation targeting a fixed parameter is an error. Definition replacement keeps
the explicit detached-binding workflow; detached bindings cannot activate against
a fixed parameter.

## Project-local standard operators

Disconnected operators and partially wired branches can remain in the authored
composition graph while being edited and saved. Only nodes with a path to the
output are prepared for playback; operators on that path must have every input
connected. Definitions, parameter types, edge ports, connection cardinality,
and acyclicity are validated even for disconnected branches.

`operators/standard.operator.donder` is editable DSL source, bundled alongside
`effects/standard.effect.donder` in new projects. The canonical copy lives in
`examples/starter/operators/standard.operator.donder`; `examples/stanford_room`
contains the same library. Each consuming YAML document explicitly imports it:

```yaml
imports:
- from:
    documents:
    - operators/standard.operator.donder
  as: operators
```

Composition nodes reference declarations such as `operators.Dim` and
`operators.HueShift`. New projects import both libraries in `project.donder`,
so the initial owned sequence has both in its document scope. Separate sequence
documents need their own imports; imports are not transitive.

| Operator | Inputs | Behavior and parameter defaults |
| --- | --- | --- |
| Max | `a`, `b` | Component-wise maximum. |
| Add | `a`, `b` | Saturating RGB addition. |
| Multiply | `a`, `b` | Component-wise RGB multiplication. |
| IntensityModulate | `source`, `mask` | Scale source by the mask's maximum RGB channel. |
| Dim | `input` | Scale by `amount = 0.5`, clamped to `[0, 1]`. |
| Invert | `input` | Complement each RGB channel. |
| Colorize | `input` | Scale `tint = #ffffff` by the input's maximum RGB channel. |
| HueShift | `source` | Add `shift = 0.0` turns to HSV hue, preserving saturation and value. Integral shifts return the sampled color unchanged. |
| Delay | `input` | Sample `seconds = 0.1` earlier; negative delay is clamped to zero. |
| Echo | `input` | Component-wise maximum of the current input and `repeats = 3` delayed copies, spaced by `seconds = 0.1` and scaled by successive powers of `decay = 0.5`. |

Echo clamps repeats to `[1, 32]`, decay to `[0, 1]`, and delay to nonnegative
seconds. Samples before the sequence are black. Its decay uses exponentiation
by squaring; copies are combined with maximum rather than addition. Parameters
are ordinary DSL parameters with their type's automation eligibility; hue shift
is an automatable float. Hue is measured in turns and wraps through `hsv`.

## HSV color intrinsics

`hue(color)`, `saturation(color)`, and `intensity(color)` each take one color
and return a normalized float. Hue is in `[0, 1)` turns; saturation and intensity
are in `[0, 1]`. Intensity is the maximum RGB channel divided by 255, which is
HSV value. Grayscale (including black) has hue zero and saturation zero.

`hsv(hue(c) + shift, saturation(c), intensity(c))` shifts hue while preserving
saturation and value. The existing `hsv` wraps hue in both directions, including
negative shifts. Colors have 8-bit RGB channels: extraction uses those quantized
channels, and `hsv` rounds its result to the nearest channel value. The standard
HueShift returns its input directly for integral shifts.

## Operator signal coordinates

An operator samples an immutable input signal by time and pixel:

- `source.at(seconds)` samples the current pixel.
- `source.at(seconds, pixel)` samples a zero-based pixel in the current fixture
  instance. For example, `source.at(seconds(), pixel_count() - 1 -
  pixel_index())` mirrors each fixture independently.
- `source.at_global(seconds, pixel)` samples a zero-based pixel in the full
  prepared layout's instance order, with each instance's pixels contiguous.
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

### Owned values and reusable references

Ownership is separate from file location. `ValueSource::Inline` contains a value
owned by its parent; `ValueSource::Reference` links a separately named reusable
object. A named object in the same file is still a reference. The shared pattern
covers project setup and sequences, setup layout/patch/controllers, fixture
geometry, curves, and gradients. Effect and operator code remains named DSL
source; instance parameters and nested clips, layers, and groups stay owned.

YAML writes an inline value as a typed mapping at the owner's field and a reusable
reference as a local or imported symbol. Inline collection members (controllers
and sequences) carry a numeric `id`, unique within their owner. These IDs survive
reordering and are not source names. Singleton slots (setup/layout/patch) need no
extra ID. Removing an owned value removes its contents; removing a reference
leaves the reusable source object intact.

`SourceIdentity` identifies a named symbol. `ObjectIdentity` adds a typed path to
an owned descendant. Patch routes and effect targets can address an owned layout
or controller without creating a source symbol:

```yaml
layout: {owner: show, path: [setup, layout]}
controller: {owner: show, path: [setup, {type: controller, id: 3}]}
```

`owner` uses the normal import scope. Each step must be a valid ownership slot;
references cannot be traversed as if they owned their targets. To share an owned
value, first give it a reusable named source. Loading validates ownership,
collection IDs, and target existence. Saving preserves nesting and list order.
Document moves remap the named root while retaining owned paths. GUI addresses
use the same paths, including when a fixture is nested inside an inline layout.

Typed ownership edits promote an owned value to a named source or copy a linked
value into its parent. Promotion rebases descendant identities and references to
them. Making a layout or controller independent also makes its patch independent
when routing must change. For the active setup, sequences targeting the copied
layout become independent before their targets change. Reusable originals remain
unchanged. Unused reusable sequences validate against their own target layouts;
sequences included in the project must target the active layout.

Ownership changes maintain local imports from typed references, including fixture,
effect, operator, curve, gradient, and route targets. A reference moved to another
document reuses or creates an explicit local import with a unique alias. Audio
is a project-relative path string; copying a sequence preserves that path and
does not copy or modify audio bytes. No audio inventory is required.

New projects begin with a single `project.donder` document containing an owned
setup, empty layout and patch, and an owned initial sequence. Source actions can
promote those values to reusable objects when sharing is needed. **Use existing
source** establishes an explicit link at any supported ownership slot. Replacing
a layout/controller follows the affected routing; incompatible targets reject
the entire edit. Source pickers offer named objects in loaded local documents; inline children are never offered as symbols.

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
- Authored YAML objects and nested schema mappings reject unknown keys, including
  fields belonging to another variant. Successful parsing checks for unconsumed
  fields through a shared mapping boundary; strict Serde leaf schemas enforce the
  same rule for curve points and gradient stops. Diagnostics identify the field
  and its value location. Document object names and parameter names are deliberate
  dictionary keys, but their values are parsed strictly. Arbitrary metadata is
  not retained or silently discarded.
- Saving retained DSL text does not project edits to compiled effect/operator
  definitions back into source. Generated child effects are derived preparation
  output, not independently editable source objects. Editing generated output
  back into arbitrary generator code is outside this contract.
- A generator's unqualified `timeline.emit Child` resolves `Child` in the
  generator definition's document, not in the calling YAML document's import
  group. Cross-file children require an explicit effect-document import (below).
  Imports do not re-export imported names, and operator documents do not support
  imports: operators currently have no corresponding cross-file call construct.
- Project save writes loaded local documents. Unreferenced files are left alone.
- Semantic round-tripping is not byte-identical round-tripping. The separate
  source-text write API writes supplied text exactly; that does not imply that a
  later typed project save preserves its presentation.

### Verification coverage

`crates/donder-project-io/tests/semantic_preservation.rs` exercises a typed edit of
`examples/starter`, full typed-project equality after save/reload, meaningful
list order, document inventories/ownership, import edges, asset references,
retained DSL text, and canonical serialization stability. It also verifies
unknown-key rejection, unused-object preservation, missing-import diagnostics,
and refusal to save inconsistent inventories without touching files.
`crates/donder-project-io/tests/roundtrip.rs` additionally checks same-named definitions in different files
through save/reload and insertion of a sequence in a new nested file.
`crates/donder-elaboration/tests/generator_source_scope.rs` checks cross-document
and local generator children, mutual imports, rejection of caller-scope lookup,
and actual starter generator emission with nonempty marks/gradients.
`crates/donder-project-io/tests/path_refactor.rs` covers import-path moves and
object identity through save/reload, directory moves, and audio path updates.

These are focused IO/preparation checks, not an exhaustive GUI action matrix,
proof for every schema field, or a rendered-output equivalence benchmark.

## Generator imports

Effect imports precede declarations and use the local document import
resolver. Local paths are **project-root relative**, just like YAML imports, not
relative to the effect file. Local DSL imports use the shared non-empty
document-list form:

```text
import bursts from ["effects/impact-burst.effect.donder"];

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

Local aliases use letters, digits, and underscores, with a non-digit first
character; keywords and `builtins` are reserved. An import can contain several
documents with unique object names. Imports are explicit, not transitive. Duplicate
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
`effects_2`.

Prepared playback bytecode does not retain generator import
tables, and no per-frame path or name resolution is introduced. Structural path
edits update explicit DSL import-path tokens and resolved identities; ordinary
saves retain the DSL text. This narrow source edit is not general AST-to-DSL
serialization.

## Validity

The canonical validator is `donder_language::validation::validate_sequence`.
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

## Composition graph editing

The graph has no inspector sidebar: operator parameters and automation controls
are inside each operator node, and layer name, color, and enabled-state controls
are inside each layer node. Layer controls are shared with the sequence's
Layers inspector. Larger parameter panels scroll inside resizable nodes without
panning the canvas; port connections remain outside the scrolling area.
Right-click empty canvas to add a layer or operator at that position. Right-click
a node, connection, or selection to delete it. There are no graph add/delete
toolbar buttons; the Delete/Backspace keyboard shortcut also remains available.
Deleting layers used by effects asks for a surviving destination layer;
the effects move there in the same transaction. Default and Output are protected.
Deleting operators detaches their automation bindings with a target-deleted reason.

Graph selection supports modifier-click, marquee selection, and Select All.
Moving or deleting a selection is one GUI history entry. Arrow keys move selected
nodes by one grid interval (Shift uses a larger step). Undo/redo restores authored
node positions, and rejected gestures restore the current document's positions.

Drag between ports to create a connection, or drag an existing wire endpoint to
reconnect it. Reconnection is a single typed GUI transaction and history entry;
invalid or stale edits leave the original connection intact. Connecting to a
single-input port replaces its previous incoming connection in the same edit.
Node refreshes retain React Flow's measured geometry, while changed port layouts
explicitly refresh handle bounds.

Node sizes and graph pan/zoom are workspace view preferences, saved per sequence
alongside the timeline viewport. Drag any node's bottom-right corner to resize it;
selection is not required and the resize target has no visible button or icon.
These preferences survive reopening the graph and restarting the desktop;
they do not change the authored sequence or create project undo entries.

## Curves

Curves are normalized, piecewise-linear values. They must contain at least one
point; each point’s position and value must be finite; positions are in
`[0, 1]` and strictly increasing. Sequence automation and
DSL curve reads use `donder_language::sampling::sample_curve`.

## Source diagnostics

Present optional values must have their declared shape. For example,
`automation_clips: wrong` is an error, not an empty clip list. Objects and nested
schema mappings are closed: unknown keys are errors. A misspelled optional field
cannot produce an accepted project that silently uses its default instead.
Parsing a selected variant consumes only that variant's fields, without a
separate allowed-key list. When parsing already fails (for example, on a missing
required field), that error may precede unknown-field diagnostics. Recovery and
source indexing may inspect partial mappings, but cannot accept a project in
place of the strict loader. Semantic validation and reference resolution remain
separate from field-shape validation.
Duration parsing is fallible and never invokes panicking duration
constructors.

The effect and operator DSL reports parse/type errors rather than changing a
bad literal to zero. Integer division produces a float. Integer negation,
addition, subtraction, and multiplication wrap at 32 bits; remainder by zero and
`i32::MIN % -1` return zero. Required DSL parameters cannot receive an
implicit type default.
Array indexing clamps negative and out-of-range indices to the first or last
element. Indexing an empty array returns the element type's default value;
`len()` still returns zero. This also applies after assigning a different
array to a local variable. `mark_at` uses its fallback for either a negative or
an out-of-range index.
Sampling a curve at NaN returns zero. Sampling a gradient at NaN or sampling
an empty gradient returns black; these rules apply to direct and parameter
sampling alike.
A `Signal.at` query at a negative, non-finite, or unrepresentable time returns
black without invoking its input signal.

`clamp(value, min, max)` and `curve_clamped(curve, position, min, max)` return
NaN when either bound is NaN or `min > max`; otherwise they clamp normally.
This applies to both literal and computed bounds, so these DSL operations do
not inherit Rust's panicking `f32::clamp` behavior for invalid bounds.

## Runtime budgets

The renderer limits a prepared sequence to 250,000 frames, generated effects
to 4,096 per preparation, and custom-operator Signal sampling to 4,096 unique
times per operator render. The DSL VM limits each invocation to 10,000 loop
iterations. Exceeding a budget returns an error; Donder does not silently clamp
or skip work.

## Authoring

- Store sequence data inline in a project or in a reusable `.sequence.donder` YAML source.
- Use `.effect.donder` for custom effects and `.operator.donder` for custom graph
  operators; both receive DSL highlighting in the desktop editor.
- Start from `examples/starter` for valid curve, effect, operator, and graph
  examples.

New Sequence creates an owned sequence inside the project by default. Advanced
settings can instead create a reusable source in the project file or a new
sequence file; source names and paths are unnecessary for owned sequences.
Creation uses the same undo/redo transaction as other GUI edits. The initial
layer and mark colors come from the desktop theme and become editable project
data when the sequence is created.

Duplicating a layout fixture or group copies its geometry into independently
owned values, including geometry that originally used reusable sources. Copies
receive new placement IDs next to the originals in the same group. Existing
routes and effect targets continue to address the originals; a copy is not
patched to hardware automatically.

### Root document

Every project has a fixed `project.donder` at its root. The reserved top-level
`workspace` block stores `format_version: 1` and a non-nil `project_id` UUID.
The UUID establishes document and object identity before imports are resolved.
The same document contains imports and exactly one project object, and may
contain other authored objects. Saving serializes workspace metadata alongside
typed content through the normal document save path.

The root document cannot be moved, renamed, or deleted through workspace
operations. Imported documents retain the normal move/rename behavior. No
configurable entrypoint or separate JSON manifest is used.
