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

Close the fixture editor. Pixel A's definition has a file of its own, so add it
again through **Add fixture** and rename its new instance to **Pixel B**. Drag B on the layout canvas to place it.
Both instances share their shapes while retaining independent effect targets.
Right-click a fixture row and choose **Edit fixture** to edit its shapes later.
Fixtures contain ordered shapes; layout groups organize instances for effect
targeting. See [fixture authoring](fixture_authoring.md).

## Route the output

From Setup, add an E1.31 controller. For a local practice receiver, set its
destination to 127.0.0.1, start universe 1, one universe and six channels.
Interface, sender and priority are under Advanced.

Open Patch and edit its outputs. Add an output from Pixel A to the controller
at first channel 1, RGB encoding. Add Pixel B at channel 4. Each pixel uses three
channels. Apply the outputs. Outputs that overlap or run past the controller's
channels are rejected, with the fixtures and channels named.

A controller's channels are counted across all its universes: with 510
channels per universe, channel 511 is the first channel of the second
universe, and an output may run on from one universe into the next.

RGBW uses four channels per pixel. Change encoding and color order on the
output; fixture geometry does not depend on that encoding. Partial outputs can
route sections of an instance to different channels or controllers.

## Add an effect

Open the sequence from the project overview. Right-click Pixel A's timeline row and add
Pulse. Set its start to 0 and duration to 60 seconds. Open preview and play:
A should light up while B stays dark. Playback does not require an audio file.

Save All, close, and reopen the project. Definitions, placements, routes, and
effects persist. GUI edits support undo and redo.

## Use a controller

Play always plays the Preview; light controllers follow only while live output
is on (the toolbar's antenna button). Live output is one switch for every
sequence, performance and project: it stays on until you turn it off, including
across restarts. Each sequence reopens with its playhead where you left it.

For live E1.31 output, configure the real receiver's address, universe, channels,
and pixel order, then enable live output and play. Setup's channel test sends a
chosen raw channel value for ten seconds and blackouts on stop or expiry. This
test bypasses the patch and does not edit the project.

For a supported ESP32 flashed with the Donder firmware, join the
controller's `Donder-XXXX` Wi-Fi network (password `12345678`), and add a
**Donder controller** to the setup. Turn on live output, then press Play: the controller plays the
sequence in sync with the audio. A controller that is
off the network or fails is listed as failed in the live-output status and the
rest still play. See [controller setup](esp32_loading.md#set-up-a-controller).

From here, the composition graph, operators and automation clips are described
in [project language](project_language.md#sequences), and writing your own effects
in [effect language](effect_language.md).
