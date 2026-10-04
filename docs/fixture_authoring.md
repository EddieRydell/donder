# Fixture authoring

A fixture contains an ordered list of editable shapes: individual pixels, lines,
polylines, arcs (including full circles), and grids. Each shape has a stable ID,
name, transform, pixel diameter, and pixel order. Layouts place reusable fixtures
and organize their instances into groups. Shapes are not separate effect targets.

## Drawing and editing

Choose a shape in the fixture sidebar or through Add shape in either right-click
menu. Enter a pixel count before drawing; grids use columns times rows.

- Pixel: click to place.
- Line: drag between endpoints.
- Polyline: click control points and press Enter to finish; Escape cancels.
- Arc: drag from its center to its starting point, then adjust the end handle or
  sweep angle. Circle uses a full turn without duplicating the first pixel.
- Grid: drag opposite corners; select its starting corner, rows or columns first,
  and straight or serpentine traversal.

Select a shape to edit its name, geometry, count, pixel diameter, and transform.
Numeric changes commit on blur or Enter; Escape restores the previous value.
Canvas handles edit control points and dimensions. Either endpoint of a line can
move independently while the opposite endpoint stays fixed. The origin handle of
an arc or grid moves its origin; dragging a pixel in the shape moves the whole shape.
Alt-drag or middle-drag pans, the wheel zooms, and Home fits the drawing.

Changing geometry preserves pixel count. A line or open arc includes both
endpoints when it contains two or more pixels; one pixel occupies its start.
Polylines distribute their count evenly over total path length, without inserting
extra pixels at corners. Repeated consecutive control points consume no length;
a completely collapsed polyline is invalid. Grids with one row or column occupy
that edge of the authored rectangle. Full circles omit the duplicate endpoint.

The shape list displays pixel counts and one-based output ranges. Drag rows to
reorder them, or use the earlier/later controls. List order sets output order;
Reverse pixel order reverses only the selected shape. Show pixel order displays
the selected shape's start, traversal, and numbers when space permits.

Convert to individual pixels replaces a shape in place with its current ordered
pixels. Those pixels can be edited separately; Undo restores the shape. Drawing,
handle gestures, reordering, conversion, and numeric edits use ordinary history.

## Precision and arrangement

Both spatial editors share a saved Snap distance and display unit (meters,
centimeters, millimeters, inches, or feet). Snap attracts drawing points and
movement anchors to the grid, nearby existing points, and guides. Fixture drawing
uses authored control points and individual pixels as targets; layout movement
can snap a placement origin to another fixture's pixels. Ctrl/Cmd temporarily
bypasses snapping. The grid coarsens at low zoom while preserving the chosen snap
increment.

Hold Shift while drawing or moving to constrain the direction to a multiple of
45 degrees. Shift makes a new grid square. Constraints take precedence over
absolute grid coordinates when the starting anchor is off-grid. Live readouts
show length, angle, and X/Y displacement in the chosen unit. Rulers and selection
width/height use the same unit. Backspace removes the last polyline vertex before
Enter finishes it.

Drag an empty part of the canvas to select all objects fully inside the box.
Shift/Ctrl/Cmd-click a list row or canvas object to add or remove it from the
selection; Ctrl/Cmd-A selects the canvas objects. A selected layout group moves
its descendants together, without moving a separately selected descendant twice.
Arrows nudge by the snap distance, Shift-arrows by ten steps. Delete removes the
selection and Ctrl/Cmd-D duplicates it. Escape cancels an active gesture.

The sidebar provides **Align and distribute** and **Repeat selection**. Alignment
uses the outer bounds of the rendered pixels, including diameter. Distribution
makes equal horizontal or vertical gaps and requires at least three objects.
Repeat creates a row/column array: the counts include the original, horizontal
and vertical steps are offsets between copies, and positive vertical steps go
up. Each arrangement, group move, or repeat is one undoable edit. Repeated layout
fixtures own independent geometry even when the originals reference reusable
sources. Repeat is limited to 1,000 selected-object copies per operation.

Under **Guides and shortcuts**, add horizontal or vertical guides and set their
coordinates numerically. Guides are saved per fixture/layout view with workspace
preferences, follow document path moves, and are not part of authored geometry
or its undo history.

## Layout and shared fixtures

The layout sidebar contains groups and fixture instances. Each instance has a
name and position/rotation/scale. Effects target instances or whole layout groups.
The list and canvas share Add fixture and Add group actions. New fixtures are
owned directly by their placement and opened for editing by default. Advanced settings,
below Name, can instead create a reusable source in this file or a uniquely named
file in the fixtures folder. Existing reusable fixtures can be added from the
Add fixture submenu. A selected fixture's source actions offer **Make reusable**
for owned geometry and **Make independent** for a link. Making a link independent
copies its geometry into that placement and leaves the reusable source unchanged.

Right-click a fixture row to edit, rename, or remove it. Removing
an inline fixture removes its owned shape data. Copying an inline fixture creates
independent geometry. Referenced fixture data remains available after removing
its placements, whether its source is in the same file or another file.
Editing a shared fixture
changes all instances referencing it. Fixtures in the current document open in a
modal; fixtures in another document open in their editor tab. The fixture modal's
Save and close button saves through the normal project save path before closing;
a failed save leaves the editor open.

The layout hierarchy supports dragging fixtures and groups. Drop onto the middle
of a group row to move inside it; the group opens while hovering. Drop near the
top or bottom of a row to reorder siblings, or onto the bottom drop area to move
to the top level. Moves preserve fixture IDs, geometry ownership, and canvas
positions, and use normal undo/redo. Groups cannot be moved into their descendants.
Reordering changes layout traversal order, and changing group membership also
changes which fixtures are included when an effect or patch targets that group.

Duplicating a fixture or group copies its geometry into independently owned
values, even when the original used a reusable source. Copies get new placement
IDs next to the originals. Routes and effect targets keep addressing the
originals; a copy is not patched automatically.

## Authored format

An owned fixture places its geometry directly inside the layout entry. It needs
no extra named source object:

```yaml
main:
  type: layout
  fixtures:
  - id: 1
    name: Front Left
    type: fixture
    definition:
      type: fixture
      elements: []
```

A reference instead uses `definition: strip` (or an imported alias). The named
source is reusable independently of where its document is stored:

```yaml
strip:
  type: fixture
  elements:
  - id: 1
    name: Bottom edge
    diameter: 0.01
    reverse: false
    transform:
      position: { x: 0, y: 0, z: 0 }
    shape:
      type: line
      length: 2
      count: 60
```

The elements list is authoritative; generated pixels are never saved alongside
it. Shape geometry and transforms use meters and degrees. Each shape and fixture
must have at most 1,000,000 pixels. Individual shapes have positive counts, while
an empty fixture is valid. IDs are unique within the fixture. Grid dimensions
multiply to its count; arcs require a nonzero sweep of at most one full turn.

## Output and ownership

Preparation expands shapes once into flat pixel buffers and resolves layout
targets to instance ranges. Playback does not generate shapes or resolve names,
imports, or groups per frame. Shape IDs and local pixel ordinals are independent
of list order and reversal.

RGB/RGBW encoding, channel order, brightness, and gamma belong to LED routes.
Routes can select pixel ranges. Changing counts or shape order changes those
ranges, so review the patch when changing fixture wiring. Effects continue to
target whole instances or groups.

Copying a layout preserves shared fixtures and instance IDs, creates its own
patch, and retargets affected sequences. Shared fixtures remain linked through
explicit local imports and can be edited at their source.
