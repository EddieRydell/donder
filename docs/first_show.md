# Create your first LED show

Create a project with **File > New Project**. Donder starts with an empty layout,
patch, and sequence, stored inside `project.data.donder`. Open the setup and
sequence from the project overview, then open Layout or Patch from the setup.
Every open object has a description bar at the top for notes about it.

## Define and place pixels

Open Layout and right-click the fixture list. Choose **Add fixture > Create new fixture**
and enter **Pixel A**; Donder stores it as the name `pixel_a`, which the text
and references use. In the fixture editor, choose **Pixel** and click the canvas.
Its default diameter is 0.01 meters; edit its position and diameter as needed.

Close the fixture editor. Select Pixel A and use its source actions to **Make
reusable**, choosing this file or a new file under Advanced settings. Add that
reusable fixture through **Add fixture** and
rename its new instance to **Pixel B**. Drag B on the layout canvas to place it.
Both instances share their shapes while retaining independent effect targets.
Right-click a fixture row and choose **Edit fixture** to edit its shapes later.
Fixtures contain ordered shapes; layout groups organize instances for effect
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

Open the sequence from the project overview. Right-click Pixel A's timeline row and add
Pulse. Set its start to 0 and duration to 60 seconds. Open preview and play:
A should light up while B stays dark. Playback does not require an audio file.

Save All, close, and reopen the project. Definitions, placements, routes, and
effects persist. GUI edits support undo and redo.

## Use a controller

For live E1.31 output, configure the real receiver's address, universe, channels,
and pixel order, then enable live output and play. Setup's channel test sends a
chosen raw channel value for ten seconds and blackouts on stop or expiry. This
test bypasses the patch and does not edit the project.

For a supported ESP32, install the bundled firmware over USB, join the
controller's `Donder-XXXX` Wi-Fi network, and add a **Donder controller** to the
setup. Claim it, then press Play: the editor uploads the sequence and starts the
controller in sync with the audio. See
[controller setup](esp32_loading.md#install-from-donder).

From here, the composition graph, operators and automation clips are described
in [project language](project_language.md#sequences), and writing your own effects
in [effect language](effect_language.md).
