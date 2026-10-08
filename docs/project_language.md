# Donder project language

A Donder project is a folder of documents written in one language. Data
documents describe what the editor edits: the project, setups, controllers,
layouts, fixtures, patches, sequences, curves and gradients. Script documents
hold effects and operators in the [effect language](effect_language.md). Both
kinds share one lexer and one set of literal rules. Project IO loads the
documents into the typed `DonderProject`, which is authoritative from then on.

## Goals

1. **Exact correspondence with the editor.** A valid data document is exactly
   one editor state, and every editor state is exactly one data document. Every
   field the text holds, including names and descriptions, can be shown and
   edited in the GUI, and the GUI stores nothing the text omits.
2. **Canonical tokens.** A state has exactly one valid sequence of tokens.
   Whitespace and line breaks are the only freedom, and saving normalizes them.
   Saving therefore never changes a token you wrote: not a number's spelling, not
   the order of fields, not an explicit value.
3. **One schema.** The Rust document types in
   `crates/donder-project-io/src/document/types.rs` are the schema. Their
   `#[derive(Data)]` (`crates/donder-data-derive`) generates decoding, encoding
   and a shape description from the same field lists.

## Documents

| Kind | File | Contains | Comments |
| --- | --- | --- | --- |
| Data | `*.data.donder` | Data declarations; the root is `project.data.donder` | No |
| Script | `*.donder` | Effects and operators, with document-local functions | `--` line comments |

A data document may mix declaration types, for example a sequence with the
curves it uses. A script may declare effects, operators or both.

Data documents have no comments, because comments have no place in the editor
state and the GUI could not keep them. Objects that deserve prose have a
`description` field instead, which the GUI shows and edits. Scripts keep
comments: the GUI never rewrites code, so their source text is kept exactly.

## Lexical structure

Shared by both kinds unless noted.

- **Whitespace** separates tokens and is otherwise insignificant.
- **Comments** (scripts only): `--` to the end of the line. In a data document
  `--` is an error.
- **Identifiers**: `[A-Za-z_][A-Za-z0-9_]*`. Names of objects, fields and
  parameters are `snake_case`; types, variants and enum options are
  `PascalCase`. The casing is a rule, not a convention: the wrong case is an
  error, so `whole_target` and `WholeTarget` cannot both appear.
- **Keywords** in data documents: `import`, `from`, `true`, `false` and `none`.
  `import` and `from` are field names where a field name is expected, as in an
  edge's `from:`. Scripts reserve their declaration and statement words
  (`effect`, `operator`, `fn`, `let`, `guard`, `if`, `else`, `for`, `in`) and
  their member words where a member starts (`param`, `input`, `sample`).
- **Integers**: `0`, `42`, `-7`. No leading zeros, no `+`, no separators.
- **Floats**: always contain a `.` with digits on both sides: `1.0`, `0.35`,
  `-2.5`. A data float must be the shortest spelling that reads back as the same
  32-bit float, so `0.6` is valid and `0.60` or `0.6000000238418579` is an error
  with a fix to `0.6`. Data has no exponents, NaN or infinity, and never accepts
  an integer where a float is expected (`1` is an error; write `1.0`). Scripts
  keep their int-to-float widening, because the GUI never reprints code.
- **Durations** (data): a non-negative decimal number of seconds followed by
  `s`: `1s`, `0.25s`, `249.753832s`. Exact to the nanosecond, so at most nine
  decimals, with no trailing zeros: `1.50s` and `1.0s` are errors with fixes.
- **Distances** (data): meters followed by `m`, exact to the micrometer, so at
  most six decimals and no trailing zeros: `0m`, `0.35m`, `-1.2m`.
- **Colors**: `#` and six hexadecimal digits. Data colors are lowercase:
  `#38bdf8`, and `#38BDF8` is an error with a fix.
- **Strings**: double-quoted UTF-8 with escapes `\"`, `\\`, `\n` and `\t`;
  nothing else needs escaping. No multi-line literal; use `\n`.
- **Paths** (data): `<` relative path `>` in imports and asset fields:
  `<effects/standard.donder>`. Paths are relative to the project root, use `/`,
  and may contain any character except `>`, `<` and line breaks.

## Data grammar

```text
document     = import* declaration*
import       = "import" name "from" path ("," path)* ";"
declaration  = TypeName name record_body
record_body  = "{" (field ("," field)* ","?)? "}"
field        = name ":" value
value        = literal | reference | list | tuple | record | named | map
record       = TypeName record_body?        -- a fieldless variant is its bare name
named        = TypeName name record_body    -- an owned collection member
list         = "[" (value ("," value)* ","?)? "]"
tuple        = "(" value "," value ("," value)* ")"
map          = "{" (name ":" value ("," name ":" value)* ","?)? "}"
reference    = name ("." name)*
literal      = integer | float | duration | distance | color | string | path
             | "true" | "false" | "none"
```

The schema, not the grammar, decides which form each field takes. Records and
variants both start with their type name: a field of type `Port` holds
`Port { ... }`, and a field whose type has several variants holds whichever one
it is, such as `E131 { ... }` or `Donder { ... }`. A variant without fields is
its bare name, `Multicast`, never `Multicast {}`; a record always has its
braces. A `map` appears only for effect and operator parameter values, whose
keys are the definition's parameter names.

**Everything is explicit.** Every field of a record is written, in schema order.
An optional value is the value or `none`. Because nothing is implied, an explicit
value can never be dropped by a save, and a document reads fully without knowing
any defaults. Writing fields out of order is an error, never a silent
reordering. Lists keep their order, which is meaningful and is the GUI's order.

A trailing comma is accepted everywhere, and the printer writes one after the
last item of every multi-line record, map or list.

The parser recovers at field, item and declaration boundaries, so one document
reports every syntax error with its range. A record cut short by a syntax error
does not also report the fields the error swallowed.

## Names and references

**Every object other objects refer to has a name**: setups, controllers, ports,
layouts, fixtures, groups, fixture definitions, fixture shapes, patches,
sequences, layers, clips, mark collections, graph operator nodes, curves and
gradients. The name is the object's only identity: references use it, and the
GUI shows and edits it. Objects nothing refers to (routes, automation clips,
edges) have no name. No document contains a numeric ID; loading assigns session
identities in document order.

| Object | Unique within |
| --- | --- |
| Top-level declarations | their document |
| Fixtures and groups | their layout |
| Fixture shapes | their fixture definition |
| Ports | their controller |
| Layers and graph nodes together; clips; mark collections | their sequence |

A layer node is named by its layer and the output node is named `output`, so a
graph node's name never collides with a layer's. The GUI derives names from what
it creates (`pulse`, `pulse_2`, `time_warp`), and a name typed in the GUI is
converted to `snake_case` (`Time Warp` becomes `time_warp`).

**References** are dotted paths resolved from where they are written:

- a bare name resolves in the enclosing scope: a declaration of the same
  document, or a layer, clip, node or mark collection of the same sequence;
- `alias.name` reaches a top-level declaration of an imported document;
- `layout.fixture` reaches a fixture or group of a layout, whatever groups list
  it, so changing group membership changes no reference;
- an owned object is reached through its owner and the owning fields:
  `show.setup.layout`, `show.setup.controllers.main`, `show.setup.layout.desk`.

The expected kind comes from the schema, so a reference to the wrong kind of
object is an error at the reference.

## Owned and shared objects

Some fields take either a reference or the object written in place, which its
owner then owns: a project's `setup` and `sequences`, a setup's `layout`,
`patch` and `controllers`, and a fixture's `definition`. Owned and shared
objects behave differently, so the two forms are two meanings, not two
spellings: an owned object is copied and deleted with its owner, and a shared one
is edited once for every user. An owned single value is a record
(`setup: Setup { ... }`); an owned collection member carries its name
(`controllers: [Controller main { ... }]`). Effect and operator code is always a
named script declaration. Parameter values, clips, layers and groups are always
owned.

The editor's source actions move values between the two forms:

- **Make reusable** promotes an owned value to a declaration, in this document or
  a new one, and rebases references to its descendants.
- **Make independent** copies a shared value into its owner, leaving the
  declaration unchanged.
- **Use existing source** links a field to a declaration.

Linked curves and gradients follow their declaration; making one independent
keeps its current value in the parameter. New sequences, fixtures and other
values are owned by default; their creation dialogs offer a reusable declaration
in this document or a new one under Advanced settings. Routing follows these
changes: making a layout or controller independent also makes its patch
independent when routes must change, and for the active setup, sequences that
target a copied layout become independent before their targets change. Imports
follow typed references, and undo and redo include document registration and
imports.

## Imports

```text
import effects from <effects/standard.donder>, <effects/impact-burst.donder>;
import layouts from <layouts/outputs.data.donder>;
```

- An import binds an alias to the top-level declarations of one or more
  documents, which must not repeat names. Paths are relative to the project root
  and cannot leave it.
- Aliases are identifiers; keywords and `builtins` are reserved. A document
  imports each target once.
- An import exposes its documents' own declarations, not what those documents
  import. Each document declares the imports it uses.
- Mutual imports are valid: the loader indexes a document's declarations before
  following its imports.
- Scripts import nothing; their functions are document-local.
- When the GUI references an object in another document, it reuses an import or
  adds one with a deterministic alias such as `effects` or `effects_2`.
- The project is the documents reachable by imports from `project.data.donder`;
  a file in the folder is not part of it until something imports it. Each
  `*.donder` file nothing reaches gets a warning, with a fix that imports it
  from the root. The fix extends the root import named for what the document
  declares (`effects`, `operators`, `scripts` for both, `layouts`, `sequences`,
  ...), or the first `alias_2`, `alias_3`, ... without a name clash. It also adds
  a document's sequences to the project's `sequences`. A setup has no fix,
  because the project names exactly one.

Prepared playback never sees paths, aliases or names.

## Schema

Each record lists its fields in order. `Option<T>` is a `T` or `none`; `[T]` is
a list.

### Project

```text
import setups from <setups/main.data.donder>;
import sequences from <sequences/empty.data.donder>, <sequences/layer_test.data.donder>;

Project starter {
  format: 1,
  id: "00000000-0000-4000-8000-000000000001",
  description: none,
  setup: setups.main,
  sequences: [sequences.empty, sequences.layer_test],
}
```

| Field | Type |
| --- | --- |
| `format` | integer, the current format marker |
| `id` | string, the project's non-nil UUID |
| `description` | `Option<string>` |
| `setup` | a `Setup` reference or `Setup { ... }` |
| `sequences` | `[Sequence reference or Sequence name { ... }]` |

The root document `project.data.donder` holds exactly one `Project`, which no
other document may declare. Its `format` and `id` establish document and object
identity before imports resolve. The root document cannot be moved, renamed or
deleted, and there is no other manifest, lockfile or asset inventory.

New projects start as a single `project.data.donder` with an owned setup, an
empty layout and patch, and one owned sequence. The root document imports the
bundled standard effect and operator libraries, so the initial sequence can use
them; other sequence documents need their own imports.

### Setup, controllers and ports

```text
Setup main {
  description: none,
  layout: layouts.outputs_layout,
  patch: patches.outputs,
  controllers: [output_controller],
}

Controller output_controller {
  description: none,
  protocol: E131 {
    source_name: "Donder Thirty Output Controller",
    bind_address: "0.0.0.0",
    priority: 100,
    mode: Unicast { destination: "192.168.7.2" },
  },
  ports: [Port { name: port_1, address: Universe { universe: 1 }, slots: 339 }],
}
```

- `Setup`: `description`, `layout`, `patch`, `controllers: [controller]`.
- `Controller`: `description`, `protocol`, `ports: [Port]`.
- `protocol` is one of:
  - `E131 { source_name, bind_address, priority, mode }` with `mode` either
    `Multicast` or `Unicast { destination }`;
  - `ArtNet { bind_address, destination, mode }` with socket addresses such as
    `"0.0.0.0:6454"` and `mode` either `Unicast` or `Broadcast`;
  - `Donder { device }`, the controller's twelve lowercase hexadecimal digits.
- `Port`: `name`, `address`, `slots`, where `address` matches the protocol:
  `Universe { universe }`, `ArtNetPort { port_address }` or
  `Output { output }` (a Donder controller's physical output, from 1).

### Layouts and fixtures

```text
Layout outputs_layout {
  description: none,
  root: [all_outputs],
  items: [
    Group { name: all_outputs, description: none, members: [output_01] },
    Fixture {
      name: output_01,
      description: none,
      definition: fixtures.vertical_113,
      transform: Transform {
        position: (0m, 0m, 0m),
        rotation: (0.0, 0.0, 0.0),
        scale: (1.0, 1.0, 1.0),
      },
    },
  ],
}

FixtureDefinition vertical_113 {
  description: none,
  shapes: [
    Shape {
      name: vertical_strip,
      diameter: 0.05m,
      reverse: false,
      transform: Transform {
        position: (0m, 0m, 0m),
        rotation: (0.0, 0.0, 90.0),
        scale: (1.0, 1.0, 1.0),
      },
      geometry: Line { length: 11.2, count: 113 },
    },
  ],
}
```

- `Layout`: `description`, `root`, `items`. `items` lists every fixture and
  group once, each `Group { name, description, members }` or
  `Fixture { name, description, definition, transform }`, where `definition` is
  a `FixtureDefinition` reference or `FixtureDefinition { ... }` owned by that
  fixture. Fixture order in `items` is placement order.
- `root` and a group's `members` name items of the layout in display order. An
  item may be a member of several groups, and of the root as well. Each list
  names an item once, a group cannot contain itself through its members, and
  every item is in `root` or some group.
- A group targets its fixtures depth-first in member order; a fixture reached
  more than once keeps its first position.
- `Transform`: `position`, an `(x, y, z)` tuple of distances within 2 km of the
  origin; `rotation` in degrees and `scale`, each a tuple of floats.
- `FixtureDefinition`: `description`, `shapes: [Shape]`.
- `Shape`: `name`, `diameter` (a distance from 1µm to 100m), `reverse`,
  `transform`, `geometry`, which is one of:
  - `Pixel`
  - `Line { length, count }`
  - `Polyline { points: [(x, y, z) distances], count }`
  - `Arc { radius, start_degrees, sweep_degrees, count, closed }`
  - `Grid { columns, rows, width, height, axis, corner, serpentine }` with
    `axis` either `Rows` or `Columns` and `corner` one of `BottomLeft`,
    `BottomRight`, `TopLeft` or `TopRight`

Lengths inside geometry (`length`, `radius`, `width`, `height`) are floats in
meters. Fixture rules are in [fixture authoring](fixture_authoring.md).

### Patches

```text
Patch outputs {
  description: none,
  routes: [
    Route {
      target: layouts.outputs_layout.output_01,
      pixels: none,
      controller: setups.output_controller,
      port: port_1,
      start_slot: 0,
      encoding: Rgb { order: (1, 0, 2) },
      gamma: 1.0,
      brightness: 1.0,
    },
  ],
}
```

- `Route`: `target` (a fixture; groups are not routed), `pixels`, `controller`,
  `port` (a port of that controller), `start_slot`, `encoding`, `gamma`,
  `brightness`.
- `pixels` is `PixelSpan { start, count }`, or `none` to route the whole target,
  including later pixel-count edits.
- `encoding` is `Rgb { order: (r, g, b) }` or `Rgbw { order: (r, g, b, w) }`,
  the byte position of each channel.

### Sequences

```text
Sequence layer_test {
  description: none,
  duration: 249.753832s,
  frame_rate: 144,
  audio: none,
  marks: [
    MarkCollection { name: wipe_marks, description: none, color: #45b7ff, times: [4s, 5s, 6s, 7s] },
  ],
  layers: [
    Layer { name: default, description: none, color: #38bdf8, enabled: true },
    Layer { name: time_warp, description: none, color: #e0414f, enabled: true },
  ],
  clips: [
    Clip {
      name: pulse,
      description: none,
      layer: default,
      start: 58.971401s,
      duration: 1s,
      target: elements.outputs_layout.all_outputs,
      scope: WholeTarget,
      effect: effects.Pulse,
      params: { gradient: gradients.ember_core_gradient, pulse_shape: [(0.0, 1.0), (1.0, 0.0)] },
    },
  ],
  graph: Graph {
    nodes: [
      LayerNode { layer: default, position: (566.45416, -474.141) },
      OutputNode { position: (1393.0885, -375.6623) },
      LayerNode { layer: time_warp, position: (444.03857, -40.094635) },
      OperatorNode {
        name: time_warp_2,
        operator: operators.TimeWarp,
        params: {},
        position: (815.0807, -62.519207),
      },
    ],
    edges: [
      Edge { from: default, to: output },
      Edge { from: time_warp, to: time_warp_2.source },
      Edge { from: time_warp_2, to: output },
    ],
  },
  automation: [
    AutomationClip {
      row: elements.outputs_layout.all_outputs,
      start: 0s,
      duration: 249.753832s,
      curve: [(0.0, 0.505), (0.103248, 0.787)],
      bindings: [NodeParam { node: time_warp_2, param: offset_seconds }],
      detached: [],
    },
  ],
}
```

- `Sequence`: `description`, `duration`, `frame_rate`, `audio` (a path or
  `none`), `marks`, `layers`, `clips`, `graph`, `automation`.
- `MarkCollection`: `name`, `description`, `color`, `times`, in time order.
- `Layer`: `name`, `description`, `color`, `enabled`. The first layer is the
  default layer, which new clips use and which cannot be deleted.
- `Clip`: `name`, `description`, `layer`, `start`, `duration`, `target` (a
  fixture or group), `scope` (`PerFixture` or `WholeTarget`), `effect`, `params`.
- **Parameters are overrides.** A parameter absent from `params` follows the
  definition's declared default, including when that default changes later; a
  parameter present keeps its value, even if it equals the default. Values are
  typed by the definition and written in its parameter order: ints, floats,
  bools, colors, enum options (`Forward`), mark collections by name, curves and
  gradients by reference or as literals, and arrays as lists. A curve literal is
  a list of `(position, value)` float tuples; a gradient literal is a list of
  `(position, color)` tuples.
- `Graph`: `nodes` and `edges`. A node is one of:
  - `LayerNode { layer, position }`, named by its layer, which appears in at
    most one node;
  - `OperatorNode { name, operator, params, position }`;
  - `OutputNode { position }`, named `output`, exactly one.

  Every node has one output, so an edge names only the input it reaches:
  `to: time_warp_2.source` names an operator input and `to: output` the output
  node. `position` is the node's place on the graph canvas.
- `AutomationClip`: `row` (the fixture or group whose automation row holds it),
  `start`, `duration`, `curve`, `bindings` and `detached`. A binding is
  `ClipParam { clip, param }` or `NodeParam { node, param }`; a detached binding
  is `Detached { binding, reason }` with `reason` either `TargetDeleted` or
  `DefinitionChanged`.

Layers produce signals, operators combine and transform them, and the single
output node is what plays. Nodes not connected to the output may remain while
you edit, and are not prepared. An operator on the output path must have every
input connected.

Every fixture or group has one clip row and one automation row. A member of
several groups shows its rows under each, and every copy holds the same clips.
An automation clip's row is only placement: moving it between rows does not
change its bindings. One clip can bind several parameters. The curve's `0..1`
value maps onto each parameter's declared range (a curve parameter's point
values), an enum's options in order, or a bool (on at 0.5). Several clips may
bind the same parameter if they do not overlap in time; preparation merges them
into one envelope. Before the first clip the parameter holds that clip's first
value, and in a gap it holds the value the previous clip ended on, so splitting
a clip leaves playback unchanged. When the GUI deletes a bound clip or operator, or
replaces its definition so a parameter no longer fits, the binding moves to
`detached` with that reason, so it can be reattached rather than silently lost.

In the editor:

- Copy and paste of clips and automation together keeps relative timing and
  target offsets, names pasted clips afresh, and remaps bindings to the copies.
  Bindings to objects outside the copied selection are dropped; a lone clip
  pastes unbound.
- A paste that does not fit is rejected as a whole. Moving, resizing, cutting or
  pasting a selection is one history entry.
- Resizing an automation clip crops it: the edge moves over fixed content,
  points outside the new window are dropped, and a cut edge gains a point at the
  value it cut through. Holding Ctrl while resizing stretches the curve instead.
  Splitting crops the clip into two clips with the same bindings.
- Curve points keep their authored order. Clicking the line adds a point,
  double-clicking a point removes it, and a dragged point stays between its
  neighbors, so a step (two points at one time) never reorders. Shift aligns
  the dragged point's time or value with a neighbor; Alt snaps clip edges and
  point times to visible marks. Committing a curve drops points that cannot
  affect sampling.
- In the graph, operator names, parameters and automation controls live inside
  each node, and layer controls inside layer nodes. Right-click to add or delete;
  deleting a layer asks where its clips should move. The default layer and the
  output cannot be deleted.
- Row heights, node sizes, graph pan and zoom and guides are workspace
  preferences, not project data, and are outside undo history.

### Curves and gradients

```text
Curve ease_down {
  description: none,
  points: [(0.0, 1.0), (0.5, 0.2), (1.0, 0.0)],
}

Gradient warm_gradient {
  description: none,
  stops: [(0.0, #fff4d6), (0.35, #ffb000), (1.0, #ff2a00)],
}
```

### Descriptions

The project, setups, controllers, layouts, fixtures and groups, fixture
definitions, patches, sequences, layers, mark collections, clips, curves and
gradients have a `description`. The GUI edits an open object's description in
the bar above its editor, and an item's with the item: a layout item's in its
name dialog, a clip's in the clip inspector, a layer's in its graph node, and a
mark collection's in the marks inspector. Text is trimmed, and empty text is
`none`.

## Scripts

Scripts are the [effect language](effect_language.md), with these rules shared
with data:

- Enum options are `PascalCase`:
  `param direction: enum { Forward, Backward } = Forward;`, compared as
  `direction == Forward`. A `snake_case` option is an error with a fix.
- Effects, operators, parameters and functions may carry a description string
  after their name, or for a parameter after its default. The inspector shows
  effect and parameter descriptions.

  ```text
  effect Pulse "A gradient that swells and fades over the clip." {
    param colors: gradient "Colors across the target.";
    param speed: float in 0.0..4.0 = 1.0 "Pulses per second.";
    sample { colors[pixel.fraction] * sin(time * speed * TAU) }
  }

  fn glow "Brightness of a soft dot." (position: float, head: float) -> float { ... }
  ```

- Scripts accept non-canonical numbers and int-to-float widening, since the GUI
  never reprints them.

## Validity

`donder_language::validation::validate_sequence` is the single sequence
validator. Project loading and checked GUI edits run it before accepting state;
preparation does not repeat it.

- Duration is positive. Frame rate is positive, and duration × frame rate is at
  most 250,000 frames. All times fit the portable 32-bit microsecond clock, and
  positive durations cannot round to zero.
- Names are `snake_case` and unique where the table above says. Timed objects
  fit within the sequence.
- Clips reference an existing layer, a compatible target and a defined effect,
  and supply every parameter that has no default and no unknown ones, each
  within its declared range. Reduction
  bounds that depend on an array or marks length must fit the 10,000-iteration
  limit with the supplied values.
- The graph has one output, typed acyclic connections, and layer nodes that
  reference distinct layers. Operator definitions, parameters, port types and
  cardinality are checked even on disconnected branches.
- Active automation bindings target existing parameters that support
  automation. A clip binds a target at most once (active or detached), and clips
  that bind the same target do not overlap. Automation curve values lie in
  0..1.

## Saving

Saving prints every project-owned data document from typed state with the
canonical printer: two-space indent; a record, list or map stays on one line
when it fits in 100 columns and otherwise puts each item on its own line; long
lists of numbers and tuples (curves, mark times) wrap densely. Because tokens
are canonical and every field is explicit, saving a loaded project rewrites no
byte of a canonically formatted document, and changes only whitespace in any
other valid one. Script text is kept exactly and never regenerated. Audio is a
project-relative path; copying a sequence keeps the path and does not copy audio
bytes. **Create Standalone Project Copy** (and `donder copy`) writes a separate
project containing the loaded sources and referenced audio.

Each file is written through `donder_project_io::atomic_write`, which replaces
it with a complete synced temporary file. That keeps any one file from being
truncated, but a multi-file save is not a crash-atomic transaction. Saving
refuses, before writing anything, a typed object with no source document, a
missing source object, or a cross-document reference with no import; it never
invents an alias or flattens a definition. All declarations in loaded documents
are typed, including unused ones. Unreferenced files are left alone.

`crates/donder-project-io/tests/project_io/exact_round_trip.rs`, `roundtrip.rs`
and `path_refactor.rs` cover byte-exact save after load, whitespace
normalization, typed edits with save and reload, list order, ownership, imports,
assets, retained script text, refusal of inconsistent saves, and import-path
moves. `schema_strictness.rs` and `diagnostics.rs` cover unknown and misplaced
fields, value types, references and diagnostic ranges.

## Implementation

- `donder_language::data` holds the syntax tree, the error-tolerant parser, the
  canonical printer and literal rules, and the schema traits (`Data`, `Record`,
  `Source`, `NamedSource`, `Reference`, `Name`, `Params`, `Meters`, `Path`).
- `donder_project_io::document` declares the document types, decodes declarations
  and encodes typed state; `loader` resolves declarations into the typed project.
  Domain validation stays with the domain types, which both loading and the
  GUI's edits call, so the text and the GUI accept exactly the same states.
- `donder-language-server` provides editor features for both document kinds;
  see [Text editing](architecture.md#text-editing).
