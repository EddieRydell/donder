# Your first show

Create two RGB pixel props, put an effect on one, and play it without an audio
file. You can complete this walkthrough using Dawn's preview before connecting
lights.

## Create the project

Choose **File → New Project...**, enter a project folder name, choose its parent
location, and press **Create**. New projects contain an empty setup and a silent
60-second sequence.

The project opens on a simple overview of its display setup and sequences. Open
the display setup row, then open **Layout**. New projects already contain an
empty layout, element tree, and patch, so no source files need to be created.
If the project file shows source text, choose **View → Toggle GUI / Text Mode**.

## Add two props

On **Layout**, expand **Add light** and fill in the form:

| Field | First prop | Second prop |
| --- | --- | --- |
| Name | Prop A | Prop B |
| Color capability | RGB | RGB |
| Shape | Line / connected lines | Line / connected lines |
| Pixels | 30 | 30 |
| Second point X (meters) | 2 | 2 |
| Bulb diameter (meters) | 0.04 | 0.04 |
| X (meters) | 0 | 0 |
| Y (meters) | 0 | 1 |
| Group | Top level | Top level |

Press **Add light** after each prop. Dawn creates its elements, shape, placement,
and preview bindings together. Each prop should report 30 pixels.

To reuse a shape, choose **Definition ? Use existing fixture**. Each added light
gets its own elements and placement while sharing that fixture definition.
**Duplicate light** has the same sharing behavior. Use **Make independent fixture
copy** when only one placement should receive later shape edits. **Remove placement**
removes its canvas placement while keeping its elements and output assignments.

## Set up an output

Return to the display setup overview and open **Controllers**. Add an **E1.31 / sACN** controller. Use local interface
`0.0.0.0`, priority `100`, and destination `127.0.0.1` for this practice project.
Set output 1 to universe `1` and `180` channels, then press **Add controller**.
Keep live output disabled while working in preview.

Return to the display setup overview and open **Patch**. Under **Assign a light
to an output**, select the controller and
output 1. Assign Prop A at start channel **1**, then Prop B at **91**, pressing
**Assign output** each time. Use RGB color order and **Replace this light's
outputs**. Each 30-pixel RGB prop occupies 90 channels; the assignments should
cover 1–90 and 91–180 without overlapping.

## Add an effect and play

Open `sequences/main.sequence.dawn`. Right-click near the beginning of Prop A's
timeline row and choose **Add Effect → Pulse**. Select the new effect, then set
its inspector **Start** to `0` and **Duration** to `60` seconds.

Use **Open preview** in the sequence toolbar, then **Play**. The clock should
advance without choosing an audio file. Prop A should light up while Prop B
stays dark. **Pause** holds the current time; **Stop** ends playback.

Choose **File → Save All** and wait for **Saved**. Close and reopen the project;
the two props, output assignments, and effect should remain. GUI changes support
**Edit → Undo** and **Redo**.

## Connect real lights

For an E1.31 receiver, replace the practice destination with that controller's
IP address, match its universe/channel configuration and pixel color order, and
apply the controller settings. Enable live output from the sequence toolbar
when ready to transmit, then play the sequence. The preview works independently
of whether a physical receiver is connected.

For supported ESP32 standalone playback, open **Export compiled sequence** from
the sequence toolbar. Select the required output ports, install the included
firmware over USB, connect the device to Wi-Fi, then **Upload and play**. The
[controller setup guide](esp32_loading.md#install-from-dawn) covers supported
hardware, saved data, playback controls, and current hardware-verification limits.

## RGBW and discrete-emitter props

The light form also supports **RGBW** and **Discrete emitters**. RGBW pixels use
four channels, including a white channel extracted from the shared RGB intensity.
Output assignment offers RGBW and GRBW order. Lights continue onto consecutive
outputs without splitting a pixel across output boundaries.

For discrete emitters, declare the emitters and map supported colors to their
levels. Include black for inactive pixels. Mappings match colors exactly; fades
or gradients can introduce colors that need additional mappings. Guided output
uses the declared emitter order; the advanced patch editor can reorder channels.

Shape editing, pixel-count changes, and duplication preserve the color capability.
Guided routes resize together, and conflicting assignments reject the entire edit.
Duplicated lights have independent shapes and need their own output assignment.

## Add dimmers or indexed controls

Under **Elements → Add dimmer, indexed control, or fixture**, create a dimmer
or indexed control and choose its cell count. Indexed controls also need named
options such as Off and On.

Under **Patching → Assign a dimmer or indexed control to an output**, choose the
control, controller, output, and first channel. Each cell uses one 8-bit channel;
all cells must fit on that output. Dimmer level 0 produces channel value 0, and
level 1 produces 255. For indexed controls, enter each option's channel value
from the device's channel chart. Dawn requires a value for every option.
ID 0 supplies the value when no control clip is active. If your options do not
include ID 0, the form asks for a separate **Inactive (no active clip)** value.
Before adding or changing option IDs on an assigned control, add the corresponding
mapping in the advanced patch editor, or remove the assignment and assign it
again after editing. Renaming an option keeps its existing mapping.

Use **Replace this control's outputs** to move its guided assignments, or
**Add another copy of this output** to mirror it onto another free channel range.
The displayed channel usage helps avoid overlaps. Custom processing stays in the
advanced patch editor. Animate the control from the sequence's **Controls** tab,
then save and reopen as with the pixel show above.

To change the cell count, expand **Edit control element**, update **Cells**, and
apply the change. Guided dimmer and indexed assignments resize at their existing
start channels, including mirrored outputs. Growing a control needs free channels
after each assignment. Dawn rejects an edit that would overlap another assignment,
exceed an output, or invalidate a clip. Custom routes and references from another
patch must be edited explicitly first.

## Add a fixture profile and fixture

Start with the fixture's channel chart and selected operating mode. Under
**Fixture Profiles**, choose **Create fixture profile** and give it an identifier
prefix. A profile describes one fixture; its channel numbers start at 1 relative
to wherever you assign that fixture on a controller.

1. Add its functions. Use **range** for a continuous control, **indexed** for
   named choices, **colorMixing** for RGB/RGBW channels, or **colorWheel** for
   wheel entries. Give indexed entries the DMX minimum and maximum from the chart;
   their ranges must not overlap. Enable **Animate within range** when an entry
   should allow a control curve within that range.
2. Set each channel's role and function. A **coarse** channel carries an 8-bit
   value; a matching **fine** channel extends that function to 16 bits. For color
   mixing, assign one **colorComponent** channel to each required component.
   Use **ignored** for channels the profile does not control.
3. Leave curves **Linear** unless the device needs another response. **Gamma**
   takes an exponent; **Custom points** lets you specify input/output levels
   between 0 and 1. For example, points (0, 0), (0.5, 0.25), (1, 1) reduce the
   midpoint while retaining zero and full output.
4. Add behavior rules when ordinary color effects should also operate a dimmer,
   shutter, wheel or prism. Choose the correct function and its levels or entry
   identifiers. Each function can have only one behavior rule. Typed sequence
   controls can override these automatic values.
5. Choose **Save fixture profile**. If validation rejects the draft, correct the
   reported field and save again. The draft stays open. Editing a saved profile
   affects every fixture that uses it; **Create editable copy** makes a separate
   profile instead.

Under **Elements → Add dimmer, indexed control, or fixture**, select **Fixture
profile**, choose the saved profile, name the fixture and select **Add element**.
Then use **Patching → Assign a fixture to an output** to choose its controller,
output and first channel. The whole fixture must fit in a free channel range.
For example, a five-channel RGBW/shutter profile starting at channel 10 uses
channels 10–14. Save, reload, and check the displayed assignment before enabling
output. Author its explicit function values in the sequence's **Controls** tab.

## Build a custom output route

Use **Patching → Advanced patch editor → Edit patch** when the guided assignment
forms do not describe the processing you need. Work in the draft, then choose
**Apply patch** to accept all changes together. Existing assignments appear in
the same graph, so preserve their nodes and connections when adding a route.

Add an element source and select its element, value type, and value count. Add
the necessary conversion filters and a controller output. Connect their node
IDs in order; ordinary single-input/single-output filters use port 0. Controller
channels start at 1, while cell offsets and node ports start at 0.

For a one-cell dimmer, a complete route is **Scalar source → Scalar channel
values → 8-bit channels → Controller output**. Set each width to 1 and choose a
free controller channel. For a color source, use **Color components**, optionally
**Component order**, then **8-bit channels**; each filter's output type and count
must match the next input. Six RGB cells produce 18 component/channel values.

Apply reports incompatible connections without accepting a partial graph. Correct
the draft and apply again, or choose **Discard draft**. If another edit changed
the project while the draft was open, discard it and reopen the current patch.
After applying, save and reload to check the resulting assignments. Custom routes
may need further edits here before the guided forms can resize or replace them.

## When something blocks an edit

To check wiring before authoring a sequence, open **Test controller channels**
under **Patching**. Select the controller and output, enter the first channel,
channel count, and an 8-bit value (0-255), then choose **Start 10-second output
test**. This sends real output directly: the selected channels receive that
value and other channels on the selected port receive zero. For example, three
consecutive RGB channels at the same value illuminate one RGB pixel equally.
Use the device's channel chart for fixture controls. This bypasses the patch and
does not change the project or preview. **Stop output** sends blackout immediately;
otherwise the test sends blackout after ten seconds. Project edits and closure
also stop it. Stop any live sequence output before starting a channel test.
While the sender finishes, the controls show **Stopping output...**. If blackout
or termination fails, Dawn reports the controller error instead of claiming the
stop succeeded. Check its connection and actual lights before starting again.

To change an existing light from RGB to RGBW, or edit its discrete emitters and
color mappings, expand **Edit color capability** on that light. Choose its new
capability and output color order, then **Apply color changes**. This applies
the chosen order to all guided outputs for the light, keeps their starting
channels, and recalculates channel counts and port spans. Ensure the additional
channels are free before expanding a capability. Undo restores the light and
its routes together. Custom routes must be edited explicitly first.

- **Read-only layout:** return to **Setup** and choose
  **Make independent layout copy**. It copies elements, placements, shapes, fixture
  profiles, and routing into your project. For the active setup, matching
  project-owned show clips follow the copied elements. For imported sequences,
  choose **File → Create Editable Project Copy...** first. Choose a parent
  location and a new folder name. Dawn copies the show, imported definitions,
  and referenced audio into a separate project and opens it for editing.
  Existing destination folders cannot be overwritten. If prompted, **Save All**
  saves current edits before copying, **Discard** copies the saved project, and
  **Cancel** leaves the current project open without creating a copy.
- **Read-only controller:** use **Create editable controller copy** in its
  membership controls. Existing assignments in this setup follow the copy.
- **Overlapping channels:** check the output's free-channel ranges, then choose
  another start channel or output. The rejected edit leaves the setup unchanged.
- **Project changed:** reopen the operation to use the current project revision.
- **Save failed:** inspect the reported path, resolve the file lock or write
  permission, and choose **Save All** again. A failed save is not a saved project.
