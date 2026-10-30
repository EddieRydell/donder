# xLights import

**File > Import xLights Show** creates a new project from an xLights show
folder, the folder holding `xlights_rgbeffects.xml` and
`xlights_networks.xml`. `donder -p <new project> import-xlights <show folder>`
does the same into a project whose setup is still empty. Sequences, effects
and the preview background are not imported.

The importer is `crates/donder-xlights`. It reproduces what xLights loads:
each attribute is read as xLights' deserializer reads it, each model type
places its nodes and numbers their channels as that type's xLights code
does, and start channels and controller outputs resolve as xLights resolves
them. Each piece of the code names the xLights function it follows. A state
the importer does not reproduce exactly is refused with its reason, never
imported differently: every attribute and child element of a model is
either read, known not to change where its lights are or which channels
they take, or refused unless it has its default value. An attribute the
importer does not know refuses the model.

## What becomes what

- Each model becomes one fixture whose pixels are xLights' nodes, in channel
  order, where xLights draws them in its 3D layout. A node of several
  lights is one pixel at their middle. Evenly spaced runs of nodes become
  `Line` shapes, runs along a circle or ellipse become `Arc` shapes, and
  any other node is a `Pixel`.
- Model groups become groups. Nested groups stay nested; submodels are left
  out, since a group holds whole fixtures.
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

Models on a controller port keep the start channel the show saved, as they
do when xLights loads a show: xLights' controller auto layout reassigns
start channels only when loading finds that one has moved, which a show
xLights saved does not.

## Size

xLights layouts are in preview units. `--pixel-spacing` sets the typical
distance between neighbouring pixels in meters, and with it the layout's
size, even when the layout has a ruler. Without it, a layout with a ruler
takes its size from the last ruler, as xLights does, and one without takes
neighbouring pixels to be 7.5 cm apart, the spacing of most pixel strings.

## What is not imported

The desktop writes `xlights-import.txt` in the new project, and the CLI
prints, every model, group member and controller that was left out or not
patched, with the reason. Models left out still take their channels, so the
models that follow them are patched as in xLights.

Any model is refused when it:

- is inactive, a shadow of another model, or in another preview;
- has strings that are not RGB or RGBW pixels, such as single-color strings;
- sets individual string start channels (`Advanced`);
- is RGBW with an RGBW handling other than R=G=B to white;
- sets the smart receiver's strings per port (`ts`) above 1, which xLights
  applies inconsistently;
- has a dimming curve from a file, or one for each color;
- has a start channel that counts from the model itself;
- has a setting the importer does not know.

Each type also refuses:

| Type | Refused |
| --- | --- |
| Single Line, Matrix, Sphere, Wreath, Star, Spinner, Candy Canes | nothing more |
| Poly Line | more than one string (`PolyStrings`); drops whose nodes xLights cannot find, on which xLights itself never finishes loading |
| Arches | layer sizes |
| Tree | a first exported strand above 1 on a vertical tree |
| Circle | layer sizes that place fewer lights than the model has |
| Icicles | a drop pattern of only gaps |
| Window Frame | sides of fewer than two lights |
| Custom | a node whose cells are not next to each other |
| MultiPoint | more than one string (`MultiStrings`); right to left numbering |

Cubes, channel blocks, images, labels and DMX fixtures are not imported.

A model's channels are not patched when its start channel does not resolve,
as xLights reports it: an unknown model, universe or controller, a chain
that loops, a `>` chain after a model on No Controller, a chain after a
group, or a chain after a model whose number of channels is unknown.

Controllers that are not created, their outputs still numbered:

- DDP, ZCPP, KiNET, OPC, Twinkly and xxx Ethernet controllers, serial
  controllers, xLights' NULL controller and player-only controllers, since
  Donder sends only E1.31 and Art-Net;
- inactive controllers;
- controllers whose address is not an IP address, whose E1.31 priority is
  outside 1 to 200, whose universes are out of range or repeated, whose
  outputs send another protocol, or with an output of no channels.

A model whose channels an earlier model in the file already uses is left
out, since Donder routes do not overlap, and a model's routes are left out
when a pixel would continue from one universe into the next.
