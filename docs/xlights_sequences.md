# xLights sequences

`donder -p <project> import-sequence <sequence> --name <name> --map <mapping>
[--audio <song>]`, where the sequence is an `.xsq` file or an `.xsqz` package
holding one, plays an xLights sequence on the project's fixtures as a new
sequence. It is the same job as xLights' own *Import Effects* with a model
mapping: a sequence made for someone else's display (a vendor's sequence) is
mapped onto this one, and each effect is drawn
again on this project's geometry. The importer is
`crates/donder-import/src/xlights/sequence`.

## Mapping

The first run, with no file at `--map`, writes one to fill in: each xLights
row that has effects, and the project's groups and fixtures to choose from.
A model's or group's row is its name; a submodel's is `Model/Submodel`, a
strand's `Model/Strand 1` and a node's `Model/Strand 1/Node 3`. Each line is the row's name, a
tab, then the fixtures and groups it plays on, separated by commas:

```text
Mega Tree	mega_tree
Arches Group	left_arches, middle_arches, right_arches
```

Lines starting with `#` are notes, and a row with no targets is left out; a
line without a tab is refused. Each
target plays the row's effects on its own, as one canvas: a vendor's group
maps best onto one group of about the same shape. Run the command again to
import.

## What becomes what

- Each effect becomes a clip of its likeness in
  [`effects/xlights.donder`](../examples/starter/effects/xlights.donder), on
  the row's targets, with xLights' settings under their xLights names and in
  xLights' units. The project imports that library; the starter ships it.
- Effects with likenesses: On, Color Wash, Shockwave, SingleStrand (Chase and
  Skips), Pinwheel (the new render method), Galaxy, Wave (Sine, Triangle,
  Square, Decaying Sine), Morph, Ripple (the old circle style), Lightning,
  Spirals, Shape (circles and stars), VU Meter (Timing Event Pulse Color and
  Timing Event Bar, on the timing track's mark collection), Meteors, Garlands,
  Fan, Bars (up and down), Twinkle, Fire and Butterfly (style 4). Effects that
  keep moving particles in xLights (Meteors, Shape, Twinkle, Fire, Lightning)
  draw the same patterns from seeded randomness instead, so they look alike
  without matching xLights pixel for pixel. Shaders have likenesses by file: `Continua
  Variation.fs` is ported line for line; `Black Cherry Cosmos.fs`,
  `BlackCherryCosmos2+.fs`, `Vortex V2.fs` and `Euro Vision Triangle
  Starfield.fs` are likenesses; any other shader plays as drifting clouds in
  its palette's colors, and the report names it.
- Left out, and counted in the report: Off, Warp (it distorts the layers under
  it), SingleStrand's FX modes (WLED's effects), VU Meter's audio types, Faces,
  Moving Head and any effect with no likeness.
- A setting a file leaves out takes the default of the xLights version that
  saved it where xLights' own loading does so: a Fan from before 2025.04 is
  unscaled, and a SingleStrand from before 2024.15 reads its old 3D fade box.
- An effect's palette is its `colors`, each checked color in order; a color
  curve becomes a gradient over the clip. The brightness slider is
  `brightness`, and the layer's fade in and fade out are `fade_in` and
  `fade_out` in seconds.
- Value curves on sliders a likeness takes as a curve (Shockwave's center,
  Pinwheel's speed, thickness and arm size, a shader's speed and offsets,
  and others) are kept; other sliders take the curve's value at the start.
- The buffer style sets where an effect draws. `Per Model` styles, overlays
  and vertical stacks play on each fixture of the target on its own; the rest
  play on the whole target. Each likeness's `space` is the buffer's layout:
  the layout itself, a single line through the pixels in order, or a column
  or row per member or per strand. Its `turn` is the buffer transform. A
  SingleStrand chase on a single line turned 90° lights the whole line, as
  xLights' one-pixel-wide buffer does. Likenesses that measure in xLights
  pixels take a layout buffer as xLights' group grid: about a pixel every
  7.5 cm, its longer side at most 400.
- Timing tracks become mark collections, one per layer, labels kept: a lyric
  track's phrases, words and phonemes are three.
- The new sequence runs at xLights' frame rate and length; `--audio`, relative
  to the project root, is its song.

## Layers

xLights mixes each layer of a row into the layers under it by the upper
effect's layer method. An imported sequence plays every clip on one layer,
where clips combine by their brightest channel, except:

- An `On` or `Color Wash` that the layers under it unmask (`2 is Unmask`)
  colors them instead of playing: each clip under it that it covers for at
  least half its length gets `tinted` and a `tint` over the clip, the upper
  effect's hue and saturation at the clip's own brightness (`2 is True Unmask`
  takes its color whole). One with nothing under it in its row is left out.
- Every other mix (Additive, Brightness, Layered, the masks and reveals) plays
  by the brightest channel, and the report counts each. Layer blur and the
  transitions other than fades are left out.

Rows mix the same way: xLights draws later rows over earlier ones, and an
imported sequence keeps the brighter.
