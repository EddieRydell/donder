# Fixture authoring

A fixture definition is an ordered list of pixels. Each pixel has a stable ID,
position in meters, and diameter in meters. Definitions cannot contain groups
or references to other definitions. Strips, arches, and matrices are pixel
arrangements, with no shape-specific engine types.

## Layout

The sidebar is a tree of layout groups and fixture instances. Groups contain
groups or instances. Each instance has a name, reusable definition, and
position/rotation/scale. Select an instance to edit those placement values below
the tree, or drag it on the canvas. Effects can target an instance or an entire group.

Right-click anywhere in the inset tree to add a group or fixture. Right-click
a row to rename or remove it; fixture rows also offer **Edit definition**.
Group arrows reflect whether their children are expanded.

**Add fixture** asks for a name and definition. Choosing an existing definition
places an instance and closes the dialog. Choosing **New definition** creates
an inline definition in the layout document, places its instance, and opens
the pixel editor. No existing definitions are required.

Definitions in the current file open in a modal; definitions in other files
open in their editor tab. Both use the same editor. Editing a shared definition
updates every instance that references it.

## Pixels

The fixture editor shows a pixel list and canvas. Add, duplicate, delete, and
move pixels earlier or later using the compact controls. Select a pixel in
either view to edit X, Y, Z, and diameter. Numeric edits commit on leaving the
field or pressing Enter; Escape restores the field's previous value. Drag a
pixel on the canvas to move it. Drag the background or hold Alt to pan; Home fits
the drawing. Edits use the normal undo/redo history, with no Apply or draft mode.

List order determines output order, independently of pixel IDs. Each layout
instance has its own contiguous pixel buffer. Groups traverse instances in tree
order. Definitions and pixels are not separate effect targets.

The current YAML format is:

```yaml
strip:
  type: fixture
  pixels:
  - id: 1
    position: { x: 0, y: 0, z: 0 }
    diameter: 0.01
  - id: 2
    position: { x: 0.1, y: 0, z: 0 }
    diameter: 0.01
```

## Output and ownership

RGB/RGBW encoding, channel order, brightness, and gamma belong to LED routes,
independently of fixture geometry. Routes can select a wiring range of pixels;
effects select whole instances or layout groups and retain parameter automation.

Copying a layout preserves shared definitions and instance IDs, creates its own
patch, and retargets affected sequences. Dependency definitions remain read-only
and must be exported/imported to stay referenced from an editable copy.

Preparation converts pixel coordinates once and resolves layout targets to
instance ranges. Playback does not resolve names, imports, or groups per frame.
