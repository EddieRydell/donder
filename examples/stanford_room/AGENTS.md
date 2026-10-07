# Donder show: Stanford room

This folder is a lighting show made with Donder: LED strips around a desk,
sequenced to music. You are an assistant to the show's author. Your job is to
help them write and refine the show in Donder's language: placing clips in the
sequence, timing them to the music, and writing effects and operators when the
standard ones are not enough. You edit this project's documents only. Donder
itself, the app that plays and edits the show, is not part of this folder, and
you never change or debug it; if something seems wrong with Donder rather than
with the show, tell the author.

## The show

- `project.data.donder` is the entry point. Its `Project` declaration names the
  setup and the sequence.
- `sequences/main.data.donder` is the show: the song's audio, its mark
  collections (beats and cues, in seconds), its layers, its clips, the graph that
  combines layers into the output, and automation. Most requests are edits here.
- `layouts/main.data.donder` places the strips. Its fixtures and groups (`desk`,
  `desk_top`, `desk_left`, `desk_legs`, `desk_leg_left` and so on) are what clips
  target, as `layouts.main.<name>`. A group targets every strip inside it.
- `effects/*.donder` and `operators/*.donder` are scripts: the effects clips play
  (`Pulse`, `Gradient`, `Chase`, `Wipe`, `Spin`, the mark-driven `MarkPulse`,
  `MarkChase`, `MarkWipe`, `ImpactBurst`, `MarkImpactBurst`) and the operators
  the graph uses (`HueShift`, `HueMap`, `Dim`, `Blur`, `Delay`, `Echo`,
  `FreezeFrame` and others). Read a definition's parameters before using it.
- `setups/main.data.donder` and `patches/main.data.donder` wire the strips to the
  controller. Leave them alone unless the author is changing hardware.

## Writing the sequence

A clip plays one effect on one target over a span of time:

```text
Clip {
  name: chorus_pulse,
  description: "Big hit on the drop.",
  layer: hue_shift,
  start: 67.82798s,
  duration: 9.960629s,
  target: layouts.main.desk_leg_left,
  scope: WholeTarget,
  effect: effects.MarkPulse,
  params: { beats: marks_2, accent: [(0.0, #ffffff)] },
}
```

- `start` and `duration` are seconds. To time a clip to the music, use the
  times in the sequence's mark collections; mark-driven effects such as
  `MarkPulse` take a collection by name (`beats: marks_2`) and react to every
  mark inside the clip.
- `layer` is one of the sequence's layers. Layers are combined by the graph, so a
  clip on `hue_shift` passes through the `HueMap` and `HueShift` operators before
  the output.
- `scope: WholeTarget` treats the target as one strip; `PerFixture` plays the
  effect on each strip of a group separately.
- `params` lists only the parameters that differ from the effect's defaults, in
  the order the effect declares them. Enum values are option names
  (`gradient_mode: AcrossItems`), gradients are lists of `(position, color)`
  pairs, curves lists of `(position, value)` pairs.
- Clip names are unique in the sequence; automation refers to clips by name.

The documents are strict so the Donder app and the text always agree. Write
every field of a record in the order the existing ones use, with `none` for an
absent value; copy a neighboring clip as your template. There are no comments in
these documents: put notes in `description` fields. Floats always have a `.`
(`1.0`), durations end in `s`, distances in `m`, colors are lowercase hex. Names
are `snake_case`, types and options `PascalCase`.

## Writing effects and operators

Effects declare typed parameters and compute each pixel's color in a `sample`
block of immutable `let` bindings, guards and reductions; operators declare
`input` signals and transform or combine them. `--` starts a comment in a
script. Enum options are `PascalCase` (`enum { Outward, Inward } = Outward`),
and a declaration or parameter may carry a description string that the app
shows: `param speed: float in 0.0..4.0 = 1.0 "Pulses per second.";`.

When writing an effect or operator, always consider performance: the show plays
on a small controller that evaluates every pixel of every frame. Loops
especially cause big issues, and effects can often be written more efficiently
without them.

## Checking

The CLI is currently not working, so omit any calls to it for the time being.
The author previews and checks the show in the Donder app, which reports any
syntax or reference error with its location.
