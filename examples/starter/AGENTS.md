# Donder show

This folder is a lighting show made with Donder. You are helping its author
write and change the show: its sequences, effects, operators, fixtures and
layout. You edit this project's documents only; Donder itself, the app that
plays the show, is not part of this folder and is not yours to change.

## The documents

- `project.data.donder` is the entry point. Its `Project` declaration names the
  setup and the sequences that make up the show, and its imports lead to every
  document the show uses.
- `*.data.donder` documents hold data: setups, controllers, layouts, fixture
  definitions, patches, sequences, curves and gradients. The Donder app edits
  these too, so keep them in the exact form described below.
- `*.donder` documents are scripts: effects and operators, such as
  `effects/standard.donder` and `operators/standard.donder`, the editable
  standard libraries.

Fixtures define pixel geometry, layouts place fixtures and groups, and patches
route pixels to controller ports. A layout lists every fixture and group once in
`items`; `root` and each group's `members` name them by display order, and a
fixture may belong to several groups (`roofs` and `left_side` can both list
`left_roof`). Every item must be in `root` or some group. A setup connects a layout, a patch and
controllers. A sequence places clips of effects on layers over time, automates
their parameters, and combines layers through operators into the output.
Directories are only organization: any declaration may live in any data
document, or be written in place inside the object that owns it.

## Writing data documents

Data documents are strict so that the app and the text always agree:

- Every field of a record is written, in the order the existing documents use,
  with `none` for an absent optional value. Copy an existing object of the same
  type as your starting point.
- No comments. Put prose in the object's `description` field:
  `description: "Warm wash for the chorus."`.
- Names are `snake_case` and unique where they are declared (fixtures and groups
  within their layout; layers, clips and mark collections within their
  sequence). Types and variants are `PascalCase`: `Clip { ... }`, `WholeTarget`.
- Literals have one spelling: floats always have a `.` (`1.0`, never `1`),
  durations end in `s` (`2.5s`), distances in `m` (`0.35m`), colors are
  lowercase (`#ff8800`).
- References are dotted names: `effects.Pulse` (an imported declaration),
  `layouts.outputs_layout.output_01` (a fixture of a layout). A document may only
  refer to declarations it imports itself:
  `import effects from <effects/standard.donder>;`.

A clip looks like this:

```text
Clip {
  name: chorus_pulse,
  description: none,
  layer: default,
  start: 58.97s,
  duration: 1s,
  target: elements.outputs_layout.all_outputs,
  scope: WholeTarget,
  effect: effects.Pulse,
  params: { gradient: gradients.ember_core_gradient },
}
```

`params` lists only the parameters that differ from the effect's defaults, in
the order the effect declares them. Enum values are the option names
(`direction: Inward`), curves and gradients are references or literals
(`[(0.0, 1.0), (1.0, 0.0)]`, `[(0.0, #ffffff), (1.0, #ff2a00)]`), and mark
parameters name one of the sequence's mark collections.

## Writing effects and operators

Scripts use Donder's effect language. Effects declare typed parameters and
produce each pixel's color in a `sample` block of immutable `let` bindings,
guards and reductions. Operators declare `input` signals and sample them to
transform or combine layers. `--` starts a comment. Enum options are
`PascalCase`: `param direction: enum { Outward, Inward } = Outward;`. A
declaration or parameter may carry a description, which the app shows:

```text
effect Glow "A soft pulse from the center." {
  param speed: float in 0.0..4.0 = 1.0 "Pulses per second.";
  sample { rgb(1.0, 0.6, 0.2) * (0.5 + 0.5 * sin(time * speed * TAU)) }
}
```

Effects run for every pixel of every frame, so keep them cheap: loops and
reductions over many items are the usual cost, and most effects can be written
without them. Read the existing effects in `effects/` before writing a new one.

`effects/vixen.donder` contains supported-mode Vixen ports. Read its comments
before using a definition: its curves use percentage values `0..100`, pixel
effects have explicit frame and coordinate settings, and it does not implement
every Vixen effect or mode. Random modes use Donder's seeded random values.

## Checking the show

From this folder, `donder --path . check` validates every document the show
reaches: syntax, imports, references, effect code, targets and assets. A
document nothing imports is not checked. A successful check does not show how
the show looks; the author previews it in the Donder app.
