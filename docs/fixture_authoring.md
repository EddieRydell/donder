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
Canvas handles edit control points and dimensions. The origin handle of a line,
arc, or grid moves its origin; dragging a pixel in the shape moves the whole shape.
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

## Layout and shared fixtures

The layout sidebar contains groups and fixture instances. Each instance has a
name and position/rotation/scale. Effects target instances or whole layout groups.
The list and canvas share Add fixture and Add group actions. New fixtures are
stored in the layout file and opened for editing by default. Advanced settings,
below Name, can instead create a uniquely named file in the fixtures folder.
Existing fixtures can be added from the Add fixture submenu.

Right-click a fixture row to edit, rename, or remove it. Removing the last use of
an inline fixture also removes its saved shape data; other layouts and nested
groups count as uses. Separate fixture files remain available for reuse.
Editing a shared fixture
changes all instances referencing it. Fixtures in the current document open in a
modal; fixtures in another document open in their editor tab.

## Authored format

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
patch, and retargets affected sequences. Dependency fixtures remain read-only
and must be exported/imported to stay referenced from the copied layout.
