# Donder show

This folder is a lighting show made with Donder. You are helping its author
write and change the show: its sequences, effects, operators, fixtures and
layout. You edit this project's documents only; Donder itself, the app that
plays the show, is not part of this folder and is not yours to change.

## The documents

- `project.data.donder` is the entry point. Its `Project` declaration names the
  layout, controllers, input devices and routes, the sequences that make up the
  show and the consoles played live, and its
  imports lead to every document the show uses.
- `*.data.donder` documents hold data: controllers, input devices, the
  layout, fixture definitions, sequences, consoles, curves and gradients. The Donder app edits
  these too, so keep them in the exact form described below.
- `*.donder` documents are scripts: effects and operators, such as
  `effects/standard.donder` and `operators/standard.donder`, the editable
  standard libraries.

Fixtures define pixel geometry, the project's one layout places fixtures and
groups, and the project's routes send pixels to controller channels. The layout lists every fixture and group once in
`items`; `root` and each group's `members` name them by display order, and a
fixture may belong to several groups (`roofs` and `left_side` can both list
`left_roof`). Every item must be in `root` or some group. The project lists its controllers and
input devices (a keyboard whose keys are buttons, or MIDI and
OSC devices with buttons and axes). A sequence places clips of effects on
layers over time, automates their parameters, and combines layers through
operators into its root node. A console is played live instead: its layers and
graph are a sequence's, and its events turn button presses and axis moves into
actions: playing, starting and stopping its clips, adding live marks and
setting clip and operator node parameters from an axis's position or a press's
velocity. Its `always` clips play for the whole session.
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
  sequence; controls within their input device; layers and graph nodes, clips,
  live mark collections within their console). Types and variants are `PascalCase`: `Clip { ... }`, `WholeTarget`.
- Literals have one spelling: floats always have a `.` (`1.0`, never `1`),
  durations end in `s` (`2.5s`), distances in `m` (`0.35m`), colors are
  lowercase (`#ff8800`).
- References are dotted names: `effects.Pulse` (an imported declaration),
  and a fixture or group of the layout is named alone: `target: output_01`. A
  document may only refer to declarations it imports itself:
  `import effects from <effects/standard.donder>;`.

A clip looks like this:

```text
Clip {
  name: chorus_pulse,
  description: none,
  target: all_outputs,
  placement: Timed { start: 58.97s, duration: 1s },
  content: Effect {
    layer: default,
    scope: WholeTarget,
    effect: effects.Pulse,
    params: { gradient: gradients.ember_core_gradient },
  },
}
```

An envelope is a clip too, drawn in its target's envelope row, whose curve
drives the parameters it binds while it plays:
`content: Envelope { curve: Curve { points: [(0.0, 0.0), (1.0, 1.0)] }, bindings: [Bound { binding: NodeParam { node: dim, param: amount }, window: (0.0, 1.0) }] }`, where `dim` is an operator node of the graph.

`params` lists only the parameters that differ from the effect's defaults, in
the order the effect declares them. Enum values are the option names
(`direction: Inward`), curves and gradients are references or literals
(`Curve { points: [(0.0, 1.0), (1.0, 0.0)] }`,
`Gradient { space: LinearRgb, stops: [(0.0, #ffffff), (1.0, #ff2a00)] }`; a gradient's
`space` is `LinearRgb`, `Hsv` or `Oklab`), and mark
parameters name one of the sequence's mark collections.

A console's effect `Clip` is the same with `placement: Played { values: [] }`, or
`Timed` to play on the session clock from its start (an envelope is always timed), and its marks parameters name the console's live mark collections. Each clip is played
either by `Play` actions (stacking up to `copies` at once) or by `Start`,
`Toggle` and `always` (one copy until stopped), not both; `Stop` stops either.
A `Move` of an axis only runs `Set`, and played `values` belong on `Press`
events. A parameter a `Set` writes holds its own value until the first `Set`, one
control sets it, and a clip parameter a `Set` writes is not also played.

## Writing effects and operators

Scripts use Donder's effect language. Effects declare typed parameters and
produce each pixel's color in a `sample` block of immutable `let` bindings,
guards and reductions. Operators declare `input` signals and sample them to
transform or combine layers. `--` starts a comment. Effects, operators and enum
options are `PascalCase`; parameters, inputs, functions and `let` names are
`snake_case`: `param direction: enum { Outward, Inward } = Outward;`. A
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
An operator that blurs, glows or spreads light along a fixture reads its
neighbors with `sum for offset in -radius..=radius around input as neighbor {
neighbor * weight }`, as `Blur` and `Bloom` do, rather than computing pixel
indices with `input[...]`. Trails and long glows that fade with distance use
`scan forward input decay d`, or `scan forward input decay d as light { light * w }`
to weight the light each pixel adds, as `Trail` and `Bloom` do; their cost
does not grow with the distance. Effects and operators see only the current
frame; nothing reads another time of a signal, so light that lingers over time
is drawn by the clips themselves, with a pulse shape or curve that fades out.

Do not write defensively. Every operation is defined for every value of its
types, so no input can fail at playback: float division by zero gives infinity
or NaN, integer `//` and `%` by zero give zero, NaN flows through arithmetic and
a NaN color channel is written as zero, array indexes clamp, curves hold their
end values (an empty curve reads NaN), empty gradients are black, and samples
outside a fixture are black. Write the plain expression; guards
such as `max(x, 0.000001)` divisors, near-zero checks or `if d > 0.0` branches
only add work to every pixel. Branch only when a case should look different, and
say why.

`effects/standard.donder` is the one effect library. Beside Pulse, Chase,
Wipe, Spin, their mark-driven versions and the wipes, it holds effects ported
from Vixen 3 (Twinkle, Butterfly, Meteors, Pinwheel and others). Their levels
and curves run `0..1` like every other effect's; sliders Vixen sets in its own
`0..100` units (speed, size, twist) keep those units. Pixel effects
work on each pixel's exact position, stretched around the target's center by
`scale_x` and `scale_y`. They look close to Vixen's effects, not identical.
Read a definition's comment before using it.

A curve is stored with values in `0..1`; a `curve in a..b` parameter maps it
onto its own range, so any curve fits any curve parameter.

## Checking the show

From this folder, `donder --path . check` validates every document the show
reaches: syntax, imports, references, effect code, targets and assets. It warns
about a document nothing imports, which the show does not use and the check
does not validate.

A successful check does not show how the show looks. `donder --path . render
<sequence> --at 12.5s` prints each fixture's colors at that time, in pixel
order, with a run of one color written once as `#rrggbb×count`. Repeat `--at`,
or give `--from 12s --to 13s --step 0.1s`, to see motion; `--target <name>`
limits the output to a fixture or group of the active layout. The author
previews the show in the Donder app.
