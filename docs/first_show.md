# Create your first LED show

Create a project with **File > New Project**. Donder starts with an empty layout,
patch, and sequence. Setup links to the native editors for each resource.

## Define and place pixels

Open Layout and right-click the inset tree. Choose **Add fixture**, enter
**Pixel A**, and choose **New definition**. In the pixel editor, click **Add pixel**.
Its default diameter is 0.01 meters; edit its position and diameter as needed.

Close the pixel editor. Add another fixture named **Pixel B**, choosing the
existing definition. Drag B on the layout canvas to place it. Both instances
share the definition while retaining independent effect targets. Right-click
a fixture row and choose **Edit definition** to change its pixels later.
Definitions contain only pixels; layout groups organize instances for effect
targeting. See [fixture authoring](fixture_authoring.md).

## Route the output

From Setup, add an E1.31 controller. For a local practice receiver, use interface
0.0.0.0, priority 100, and destination 127.0.0.1. Give output 1 universe 1
and six channels.

Open Patch and edit its routes. Add a route from Pixel A to output 1, first
channel 1, RGB encoding. Add Pixel B at channel 4. Each pixel uses three channels.
Apply the routes. Overlap or insufficient port capacity rejects the edit.

RGBW uses four output channels per pixel. Change encoding and channel order on
the route; fixture geometry does not depend on that encoding. Explicit wiring
ranges can route sections of an instance to different ports.

## Add an effect

Open sequences/main.sequence.donder. Right-click Pixel A's timeline row and add
Pulse. Set its start to 0 and duration to 60 seconds. Open preview and play:
A should light up while B stays dark. Playback does not require an audio file.

Save All, close, and reopen the project. Definitions, placements, routes, and
effects persist. GUI edits support undo and redo.

## Use a controller

For live E1.31 output, configure the real receiver's address, universe, channels,
and pixel order, then enable live output and play. Setup's channel test sends a
chosen raw channel value for ten seconds and blackouts on stop or expiry. This
test bypasses the patch and does not edit the project.

For supported ESP32 standalone playback, choose **Export compiled sequence**,
select output ports, install the bundled firmware, provision Wi-Fi, and upload.
See [controller setup](esp32_loading.md#install-from-donder).

Dependency resources remain read-only. Setup can create an editable layout or
controller copy; shared dependency definitions must be exported to remain
referenced. To edit a whole imported project, use **Create Editable Project Copy**.
A rejected edit leaves the accepted project intact. A save failure reports the
path and remains unsaved until corrected.
