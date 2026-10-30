# xLights import

**File > Import xLights Show** creates a new project from an xLights show
folder, the folder holding `xlights_rgbeffects.xml` and
`xlights_networks.xml`. `donder -p <new project> import-xlights <show folder>`
does the same into a project whose setup is still empty. Sequences, effects
and the preview background are not imported.

The importer is `crates/donder-xlights`. It reads the files as xLights 2026
writes them, including files from before the 2026.04 serializer, which keep
model sizes in `parm1`, `parm2` and `parm3`.

## What becomes what

- Each model becomes one fixture whose pixels are xLights' nodes, in channel
  order, where xLights draws them. A node of several lights is one pixel at
  their middle. Evenly spaced runs of nodes become `Line` shapes, runs along a
  circle or ellipse become `Arc` shapes, scaled for an ellipse, and any other
  node is a `Pixel`.
- Model groups become groups. Nested groups stay nested; submodels are left
  out, since a group holds whole fixtures.
- E1.31 and Art-Net controllers become controllers with one port per universe.
  Controllers of other protocols, such as DDP and xLights' NULL controller,
  are left out, and so are the routes of models on them.
- Each model's channels become one route per universe. Start channels in every
  xLights form resolve, including those that follow another model.

## Size

xLights layouts are in preview units. `--pixel-spacing` sets the typical
distance between neighbouring pixels in meters, and with it the layout's
size, even when the layout has a ruler. Without it, a layout with a ruler
takes its size from the ruler, and one without takes neighbouring pixels to
be 7.5 cm apart, the spacing of most pixel strings.

## What is left out

The desktop writes `xlights-import.txt` in the new project, and the CLI
prints, every model, group member and controller that was left out or not
patched, with the reason:

- models of types without pixels (DMX fixtures, images, labels) and of types
  not imported yet (candy canes, spinners, cubes, channel blocks);
- strings that are not RGB or RGBW pixels, such as single-color strings;
- settings not imported yet: individual string start channels, arch layers
  and poly line segments, drops and strings;
- inactive models and shadow models;
- models placed by an xLights from before 2019, whose positions current
  xLights no longer reads;
- a model whose channels an earlier model in the file already uses, since
  Donder routes do not overlap;
- routes whose pixel would continue from one universe into the next.
