# Profile import

**File > Import Profile** creates a new project from the layout, controllers
and patch of an xLights or Vixen profile. `donder -p <new project>
import <folder>` does the same into a project whose setup is still empty;
the folder's contents say which program it is from. Sequences, effects and
preview backgrounds are not imported.

The importer is `crates/donder-import`. Each source has a reader that
reproduces what that program loads, and one builder makes the result the
project's setup: each fixture's pixels sit where the program draws them, in
channel order, and its channels become routes on the controller ports that
carry them. A state a reader does not reproduce exactly is refused with its
reason, never imported differently.

The desktop writes `import-report.txt` in the new project, and the CLI
prints, everything that was left out or not patched, with the reason.

## xLights

An xLights show folder holds `xlights_rgbeffects.xml` and
`xlights_networks.xml`. The xLights reader reproduces what xLights loads:
each attribute is read as xLights' deserializer reads it, each model type
places its nodes and numbers their channels as that type's xLights code
does, and start channels and controller outputs resolve as xLights resolves
them. Each piece of the code names the xLights function it follows. A state
the importer does not reproduce exactly is refused with its reason, never
imported differently: every attribute and child element of a model is
either read, known not to change where its lights are or which channels
they take, or refused unless it has its default value. An attribute the
importer does not know refuses the model.

### What becomes what

- Each model becomes one fixture whose pixels are xLights' nodes, in channel
  order, where xLights draws them in its 3D layout. A node of several
  lights is one pixel at their middle. Evenly spaced runs of nodes become
  `Line` shapes, runs along a circle or ellipse become `Arc` shapes, and
  any other node is a `Pixel`.
- Model groups become groups, and nested groups stay nested.
- A model with submodels is split along them: its pixels in the same set
  of submodels become one fixture, named after the submodel that is
  exactly it, else the model and a number. The model and each submodel of
  several fixtures become groups of them, so groups and effects that name
  a submodel (`Model/Submodel`) find it. A submodel defined by a buffer
  rectangle, rather than node ranges, is left out, and so is a Keep XY
  submodel naming nodes the model does not have.
- E1.31 and Art-Net controllers become controllers with one port per
  universe.
- Each model's channels become one route per universe.
- A model's dimming curve that is one brightness and gamma for all its
  colors (`ModelBrightness`, or a `dimmingCurve` with an `all` entry) sets
  its routes' brightness and gamma. xLights truncates the curve to whole
  steps where Donder rounds, so an output can differ by one step in 255.
  The brightness and gamma of a `ControllerConnection` are uploaded to the
  controller, which applies them, so they are not imported.

Only models in xLights' default preview, or in all previews, are placed;
the coordinates of other previews are their own.

A networks file in xLights' older form, `<network>` elements without
controllers, is grouped into controllers as xLights converts it, and each
model start channel `N:k`, channel `k` of the Nth `<network>`, is converted
with it, as xLights converts it when it opens the show.

A show xLights saved keeps its start channels, models on controller ports
included: xLights' controller auto layout reassigns start channels only
when a model has moved on load, and in a saved show none has.

### Size

xLights layouts are in preview units. `--pixel-spacing` sets the typical
distance between neighbouring pixels in meters, and with it the layout's
size, even when the layout has a ruler. Without it, a layout with a ruler
takes its size from the last ruler, as xLights does, and one without takes
neighbouring pixels to be 7.5 cm apart, the spacing of most pixel strings.
A ruler in units xLights does not name fails the import unless
`--pixel-spacing` is set.

### What is not imported

Models left out still take their channels, so the models that follow them
are patched as in xLights.

Any model is refused when it:

- is inactive, a shadow of another model, or in another preview;
- has strings that are not RGB or RGBW pixels, such as single-color strings;
- sets individual string start channels (`Advanced`);
- is RGBW with an RGBW handling other than R=G=B to white;
- sets the smart receiver's strings per port (`ts`) above 1, which xLights
  applies inconsistently;
- has a dimming curve from a file, or one for each color;
- has a start channel that counts from the model itself;
- has more lights than a Donder fixture holds, 1,000,000, or settings that
  would have the importer count more than that of anything;
- has a setting the importer does not know.

Each type also refuses:

| Type | Refused |
| --- | --- |
| Single Line, Matrix, Sphere, Wreath, Spinner, Candy Canes | nothing more |
| Star | a layer of more lights than the model has |
| Poly Line | any number of strings (`PolyStrings`) other than 1; drops whose nodes xLights cannot find, on which xLights itself never finishes loading |
| Arches | layer sizes |
| Tree | a first exported strand above 1 on a vertical tree |
| Circle | layer sizes that place fewer lights than the model has |
| Icicles | a drop pattern of only gaps |
| Window Frame | sides of fewer than two lights |
| Custom | a node whose cells are not next to each other |
| MultiPoint | any number of strings (`MultiStrings`) other than 1; right to left numbering |

Cubes, channel blocks, images, labels and DMX fixtures are not imported.

A model's channels are not patched when its start channel does not resolve,
as xLights reports it: an unknown model, universe or controller, a chain
that loops, a `>` chain after a model on No Controller, a chain after a
group, or a chain after a model whose number of channels is unknown.

Controllers that are not created still have their outputs numbered:

- DDP, ZCPP, KiNET, OPC, Twinkly and xxx Ethernet controllers, serial
  controllers, xLights' NULL controller and player-only controllers, since
  Donder sends only E1.31 and Art-Net, and controllers whose protocol is not
  one xLights knows;
- inactive controllers;
- controllers whose address is not an IP address (or, for E1.31,
  `MULTICAST`), whose E1.31 priority is
  outside 1 to 200, whose universes are out of range or repeated, whose
  outputs send another protocol, or with an output of no channels or of
  more than 512.

A model whose channels an earlier model in the file already uses is left
out, since Donder routes do not overlap, and a model's routes are left out
when a pixel would continue from one universe into the next.

## Vixen

A Vixen 3 profile folder holds `SystemData/SystemConfig.xml`, with the
element tree, controllers and patching, and `SystemData/ModuleStore.xml`,
with each module's settings, including the preview's shapes. The Vixen reader
follows Vixen 3 at commit `e111ae41963abfa48b70f884445950e94e432e02`, and
each piece of the code names the Vixen code it follows.

### What becomes what

- The profile's one enabled preview places the fixtures; with several
  enabled, `--preview <name>` picks one. Each of its shapes becomes one
  fixture, named after the element group whose elements are exactly the
  shape's, else the shape's own name, else its type.
- A shape is split along its parts: the element groups whose imported
  elements all lie in the shape without being all of it. The shape's
  pixels that are in the same set of parts become one fixture, so a mega
  tree becomes its strings and concentric stars each ring's points, and
  every part is a group of whole fixtures.
- A shape's pixels sit where the shape's layout puts them when Vixen loads
  the preview, without the rounding to whole preview units Vixen does for
  drawing, so evenly spaced lights stay evenly spaced. Preview units are the
  preview background's pixels; neighbouring pixels are taken to be 7.5 cm
  apart unless `--pixel-spacing` is set.
- A pixel is its element's channel through a color breakdown that mixes
  red, green and blue, whose three outputs are patched to consecutive
  channels of one universe; their order is the pixel's color order.
- E1.31 controllers become controllers with a port for each active universe.
  A universe sends the controller's outputs from its start, as Vixen does.
- An element group becomes a group when its elements are exactly a set of
  whole fixtures. It holds its imported child groups and the fixtures of its
  other elements.

### What is not imported

A shape is left out when:

- it is a Net, a Flood or a moving head, or rotated;
- one of its settings is not reproduced: per shape, settings Vixen itself
  cannot lay out, such as a line of no length or a star no more than 10
  units across, and child strings that do not match the shape;
- it has a pixel with a saved high-precision location (outside a custom
  prop);
- none of its pixels is linked to an element, or an element it lights is
  also lit by another pixel. Standard strings, whose lights all share one
  element, are left out this way.

A fixture's routes are left out when one of its elements is not patched as a
pixel: unpatched, patched straight to one channel, through a dimming curve
or another filter, through a breakdown that does not mix colors, sends RGBW
(Vixen mixes white another way than Donder) or 16-bit values, or whose
colors are not consecutive channels of one active universe.

Controllers that are not created: other output modules (DDP, DMX, Renard,
serial and the like), disabled controllers, and E1.31 controllers whose
address is not an IP address, that send blind, whose priority is outside 1
to 200, or whose universes are out of range, repeated, overlapping or of
more than 512 slots.

Groups that hold part of a shape and reach outside it are left out, since
Donder groups hold whole fixtures, and so are groups with no imported
elements.
