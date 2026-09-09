# First-class setup and show authoring

## Goal

A new user can create a project, add two pixel props, configure an E1.31 or
Art-Net controller, assign the props to outputs, add an effect, play without
audio, and save/reopen without editing YAML. The broader goal includes fixture
profiles, typed controls, advanced patching, editable dependency copies, and
embedded deployment and playback.

## Implementation sequence

1. Correct setup identity and mutation ownership. A GUI request must operate on
   its requested setup, and every changed object must be project-owned.
2. Complete pixel setup authoring: create, duplicate, group, reorder, remove,
   shape, position, and assign lights; configure controllers and output ports.
3. Make sequence transport work independently of audio and expose a useful
   output-test workflow with explicit activation and actionable errors.
4. Complete fixture-profile, scalar/indexed/fixture control, and advanced patch
   authoring with the same typed transaction and source-ownership contracts.
5. Provide explicit editable-copy workflows for dependency-owned setup data.
6. Complete embedded export selection, provisioning, upload, persistent state,
   and playback controls. Hardware evidence remains separate from host checks.
7. Review diagnostics and verify the complete new-user journey, persistence,
   undo/redo, and the existing automated gate.

## Design constraints

Each user action updates all related typed objects in one DesktopState candidate
transaction. Domain relationships belong to dawn-language, source inventories
and imports to dawn-project-io, and GUI DTO conversion and interaction to the
desktop. Existing patch graphs remain authoritative; convenient output
assignment must not introduce a second routing model. Shared or dependency-owned
objects require explicit handling, never hidden mutation. Errors must explain
the conflicting assignment or reference. No YAML mutation in GUI code, no
compatibility layer, and no new tests without an explicit request.

## Status

See [the requirement-oriented acceptance review](authoring_acceptance_status.md)
for current evidence, verification limits and remaining work. The entries below
are implementation history and can include gaps closed by later entries.

The software goal is complete. On 2026-09-08, the user explicitly excluded physical acceptance from this goal; hardware testing does not block completion.

### File-first setup authoring

New projects now start with a real, empty layout document rather than placing
the element tree and preview layout in the setup document. The generated project
uses `setups/main.setup.dawn`, `layouts/main.layout.dawn`, and
`patches/main.patch.dawn`; the project overview opens first.

The setup GUI is an index of its typed YAML references. Layout, element tree,
patch, fixture definition, attached controller, and fixture-profile rows show the
backing document and object key. Opening a row resolves the source module, file, and object before selecting
its native GUI. References to another document open that document in a tab.
Objects in the current document open in a modal containing the same resource
editor. Dependency sources use that editor in read-only mode; source links remain
available, while text and GUI mutations are rejected. A fixture editor receives
one resolved definition, and its edit commands target that definition directly.

Curve and gradient parameters use the same source-aware controls for single
values and array entries. Edit source opens the shared definition; making an
independent copy explicitly replaces only that value with inline data. Editing,
reordering, or removing another array entry preserves existing links, and empty
arrays keep their declared editor type.

The layout embeds the native element-tree editor beside its canvas. Opening
Elements separately uses that same hierarchy component. Tree commands address
the tree directly, including guided control/output resizing, without finding a
setup that happens to reference it. Light creation belongs to Layout and reuses
the fixture editor's geometry fields.
Shared fixture edits keep their source identity. Changing the pixel count updates
complete color-element placements and their guided outputs across the loaded
layouts; ownership, route conflicts, and sequence references still constrain the
transaction. Custom or mixed bindings require explicit rebinding. Duplication
creates new elements with the same definition; copying the definition is a
separate action.

GUI creation is file-first. Adding a pixel fixture creates a separate
`fixtures/<name>.fixture.dawn`; creating a fixture profile or controller creates
its own file under `fixture-profiles/` or `controllers/`. Editable layout copies
create a layout bundle plus separate patch, fixture, and fixture-profile files.
Controller copies likewise create separate controller and patch files. Source
imports are added through `dawn-project-io`, while all edits continue to mutate
the typed project in one `DesktopState` transaction.

Implemented in the first increment:

- Setup projection and mutations use the requested setup identity. Element,
  preview, patch, and controller writes enforce project ownership. Canonical
  project validation checks every loaded setup.
- New RGB lights can be lines, arches, or rings, with pixel count, dimensions,
  bulb diameter, position, and parent group. Creation registers the prop source
  object and updates the element tree and preview bindings in one transaction.
- Controllers can be created and configured as E1.31 or Art-Net, including
  network settings and output ports. Pixel assignment creates normal typed patch
  nodes, splits across consecutive ports, and supports RGB/GRB order. Output
  destinations can be removed while retaining shared routing branches.
- Groups, reparenting, sibling order, RGB light duplication, and deletion are
  exposed in the setup editor. Duplicates receive independent prop definitions
  and no output assignment. Deletion rejects remaining clip references and
  shared preview/group routes; it removes exclusive preview placements and
  destinations. Unused prop definitions remain available as authored resources.
- Audio-free sequences use an explicit monotonic clock for play, pause, seek,
  stop, and end-of-sequence behavior. Missing/broken audio is not converted to
  silent playback. Live-output waiting accounts for render/send time.
- New-project locks use the configured package registry.

The first increment, including the lifecycle additions, passed `cargo fmt` and
the full `pnpm check`. No new tests were added; these checks exercise existing
coverage and do not replace a manual new-user or physical-controller walkthrough.

Second increment:

- Existing RGB lights expose line/connected-line, arc/ring, and individual-point
  geometry, pixel count, bulb diameter, and three-dimensional position. Editing
  a shared or dependency-owned shape creates an independent project-owned
  definition for that placement.
- Pixel-count changes update the element, complete preview bindings, and simple
  RGB output routes atomically. The route planner handles consecutive output
  ports, color order, and multiple independent destinations, preserving existing
  patch node identities where possible. Capacity and overlapping-channel errors
  reject the candidate without changing the accepted project.
- Output assignment can replace a light's destinations or add another one.
  Recovery of existing assignments recognizes only complete RGB pipelines;
  shared, partial, group, or custom processing routes need explicit editing.
  Shape changes that retain pixel count preserve custom routing.

The second increment passed `cargo fmt` and the full `pnpm check`, including
binding generation, frontend validation/build, existing tests, and Rust Clippy.
No tests were added or changed. Manual workflow and hardware verification remain
outstanding. The frontend build still reports its bundle-size warning.

Third increment:

- Controller definitions can be attached to a setup and removed from it while
  remaining available for reuse. Removal with assigned outputs is an explicit
  action; it removes those sinks and their exclusive upstream branches in the
  same transaction. Bulk output removal rejects a patch shared by another setup.
- The controller cards and output-assignment form show occupied/free channel
  ranges derived from the current patch. Reusing a controller uses the canonical
  source-reference/import path, with dependency visibility enforced.

The third increment passed `cargo fmt` and the full `pnpm check`. Existing tests
were run without adding or modifying tests. The new controller controls still
need manual workflow verification.

Fourth increment:

- Advanced patch editing uses a typed local draft with one atomic Apply action.
  Users can create/remove sources, every filter kind, controller outputs, and
  connections; edit cell spans, widths, color capabilities, custom dimming curves,
  indexed mappings, component order, byte order, and fixture-profile references.
  Node identifiers are preserved. Source ownership and import visibility use the
  existing IO path, and accepted edits use normal persistence and undo history.
- Drafts capture their starting GUI request. A newer project revision blocks
  application and leaves the draft visible until explicitly discarded. The old
  per-connection mutation commands were removed; routing inspection is collapsed
  behind a separate read-only view.
- Source selection/type validation is shared by project validation and output
  preparation. Discrete color capabilities and dimming curves use canonical
  validation, including custom curve ordering. Patch validation errors shown by
  GUI edits describe missing connections, incompatible widths/types, invalid
  ports, cycles, and overlapping destinations.

The fourth increment passed `cargo fmt` and a final full `pnpm check` after the
last draft-revision guard. No tests were added or modified. Automated checks do
not establish manual usability or exercise every new patch-editing path.

Fifth increment:

- Scalar/dimmer, indexed-option, and fixture elements can be created, edited,
  grouped, duplicated, and removed through setup controls. Indexed option IDs and
  names and fixture-profile references are typed fields. Element creation shares
  one domain insertion path with groups and pixel lights. The old isolated
  cell-count command was replaced by the complete control-element edit.
- The explicit `scalar_to_components` patch filter carries scalar values into
  channel-component processing. It is available in the advanced editor, the
  authored format, preparation, and portable runtime; dimming/scaling and 8/16-bit
  channel conversion can now follow a scalar source. Prepared-sequence wire
  format is current marker 6; local upload artifacts must be regenerated.
- Control target resolution is shared by project validation and preparation.
  Missing options/functions/entries, incompatible values, and unsupported range
  curves are rejected before accepting edits. Conflicts are checked against
  resolved cells and fixture functions, including overlapping group and leaf
  selections. Control clip creation/value editing is still outstanding.

The fifth increment passed `cargo fmt`, full `pnpm check`, the standalone
`dawn-runtime` check with default features disabled, and the ESP32 release check
for `loader` with `i2s-output`. Existing tests were run without adding or modifying
tests. No hardware was flashed and no physical output behavior was measured.

Sixth increment:

- The sequence Controls inspector supports creation, editing, duplication,
  retargeting, and deletion of control clips. Timing, target/cell range, and value
  are applied together. Drafts capture the displayed project revision and cannot
  overwrite newer edits. The old isolated move/resize commands were removed.
- Control values are fully typed: normalized constants/curves, indexed options,
  fixture entries with optional range curves, colors, and gradients. Curve,
  gradient, and color editing reuse the existing editors and CSS-backed theme.
- The language layer supplies available control channels. Group channels expose
  the shared functions and intersected option/entry IDs of their leaves; range
  animation is offered only when every affected entry supports it. GUI projection
  and mutation share curve/gradient DTO conversion. Invalid gradient positions
  and empty fixture entry lists are rejected by canonical validation.
- Control editing currently lives in the inspector. Dedicated timeline display
  and gestures and the manual authoring walkthrough remain outstanding.

The sixth increment passed `cargo fmt`, full `pnpm check`, and the ESP32 release
check for `loader` with `i2s-output`. Existing tests were run without adding or
modifying tests. No hardware was flashed, and the new inspector workflow has not
yet been manually exercised.

Next work:

- Expose control clips on the timeline and simplify fixture routing.
- Make the common setup flow clearer and manually verify the advanced editor.
- Complete dependency copy flow, explicit output testing, embedded
  deployment/persistence/transport, and final manual workflow verification.

Seventh increment:

- Setup now provides a fixture profile editor for functions and tags, indexed
  entries and DMX ranges, RGB/RGBW color mixing, channel roles and curves, and all
  four behavior rules. Profile edits preserve identifiers and apply atomically
  through the existing setup transaction, undo, validation, and save paths.
- Dependency profiles remain read-only; an explicit editable copy creates a
  project-owned profile that can be selected for a fixture element. Copies do not
  retarget existing elements. New profiles are registered through source ownership
  APIs in the setup document; GUI editing does not manipulate YAML.
- Invalid profile errors identify missing functions, channels, entries,
  overlapping ranges, and invalid curves or behaviors. Canonical validation now
  rejects behaviors whose produced control value is incompatible with the target
  function. Drafts retain the originating project revision and reject stale saves.
- Manual fixture editing, save/reopen, and output verification remain outstanding.

The seventh increment passed `cargo fmt` and full `pnpm check`, including frontend
type checking, lint, production build, and existing Rust tests and Clippy. No tests
were added or modified. These checks do not establish manual fixture-editor or
physical output behavior.

Eighth increment:

- Fixture output assignment has a guided form: select a fixture, controller,
  output, and first channel. The profile determines the channel span. The domain
  authoring path inserts a fixture-state source, profile encoder, and sink as one
  edit. Existing output usage is visible in the form.
- The assignment rejects unattached controllers, insufficient contiguous output
  capacity, and channel overlaps. Each new route can be removed independently;
  adding another route mirrors the fixture. Replacement currently uses the
  explicit remove-and-add workflow.
- Source visibility for the element tree, controller, and profile is established
  through the existing IO import path. No YAML editing or new dependencies were
  introduced. Manual assignment and physical fixture verification are outstanding.

The eighth increment passed `cargo fmt` and full `pnpm check`. No tests were added
or modified. `pnpm --dir apps/desktop tauri build --debug --no-bundle` also passed,
producing `target/debug/dawn-desktop.exe` for the manual desktop walkthrough. No
frontend development server was started.

Desktop walkthrough, first pass:

- Launched the packaged debug app and used its File > New Project dialog and
  native folder picker to create `target/authoring-walkthrough`. The app reported
  Project checked and Saved, and the expected project documents were present.
- The first-run experience opened `project.dawn` in the text editor. Project
  creation now opens the typed root setup's document and selects GUI mode after
  a successful load. This uses the loaded setup identity rather than a template
  filename. Invalid loads retain the existing diagnostic flow.
- The full two-prop/controller/effect/playback/save/reopen walkthrough remains
  outstanding. This first pass verified project creation only; it did not send
  live output or change the starter example. The walkthrough app was closed.

The initial-screen correction passed `cargo fmt` and full `pnpm check`. No tests
were added or modified. Verification in a rebuilt desktop app is still pending.

The second desktop pass exposed a deadlock in that initial correction:
`request_transition` already owns the authoring mutex, while the public mode
switch tries to acquire it again. Creation now uses the existing locked settings
update, and its helper is named `create_new_project_locked` to make the ownership
explicit. The nonresponsive walkthrough process was stopped. The new project
files were created before the deadlock; no authored content was lost.

The lock correction passed the full check and packaged build. A fresh project at
`target/authoring-walkthrough-verified` then opened Display Setup and remained
responsive. Through the actual UI, the starter element was renamed Prop A and
resized to 30 pixels; the controller capacity was changed to 512 channels; Prop B
was added with 30 pixels at a different position. The two assignments occupy
channels 1-90 and 91-180. Undo removed the second assignment, redo restored it,
and the app reported Saved. Saved typed documents were inspected read-only.

Further walkthrough findings:

- Quick Open and Explorer used text-navigation commands, overriding GUI mode.
  Ordinary file opening now preserves the selected mode; diagnostic/search
  navigation continues to open text at a source location.
- Adding Pulse failed because required curves and gradients were initialized
  with empty VM storage defaults. Authored initialization now constructs a valid
  ramp curve, a constant gradient with a CSS-supplied initial color, and a single
  initialized element for arrays. Required mark collections remain explicit.
  Effect creation, script changes, and operator creation share this constructor.
- Corrupted context-menu arrow text was replaced with the existing icon component.
- Full verification of the resource initialization change is pending: three
  existing test constructors need the new initial-color argument. Permission for
  those mechanical updates has been requested under the no-test-edit rule. No
  tests have been modified. Playback, preview, and save/reopen verification are
  still outstanding. The walkthrough application was closed.

Packaged playback verification also found that CSS minification shortened the
initial white token to `#fff`, which the project's six-digit color parser rejects.
The CSS runtime bridge and color picker now share opaque hex normalization. This
keeps project colors valid in packaged builds without adding palette literals or
loosening the project format. Playback verification is being repeated with this
correction.

The next packaged attempt reached serialization and found a missing element-tree
import in a new sequence. Effect creation and explicit retargeting now establish
that reference through the same IO visibility API used by control clips. Failed
attempts left the sequence unchanged. Full checks still stop at the three test
constructors awaiting authorization for their mechanical API update.

Verified packaged desktop workflow:

- In the New Project flow, the included starter pixel was resized into Prop A
  with 30 pixels, and Prop B was created with 30 pixels. Both were assigned to the
  setup controller, using channels 1-90 and 91-180. This exercised the current
  starter template, not an entirely empty setup.
- Pulse was added to Prop A through the timeline menu, then edited in the
  inspector to start at zero and last 60 seconds. The resulting source contains
  the required tree import and valid inline curve/gradient values.
- With no audio file, Play advanced the clock to 0:18 and then 0:36. The native
  preview displayed the lit 30-pixel prop and reported 60 FPS. Pause held at 0:36.
  Screenshot evidence is in `target/authoring-preview.png`.
- Save All reported success. After closing and restarting the app, the effect
  inspector showed Start 0 and Duration 60. Quick Open preserved GUI mode when
  reopening setup, which still showed both 30-pixel props and both output ranges.
- No project YAML was manually edited in this walkthrough. Live network output
  was disabled, and no physical fixture behavior was measured. The app and its
  preview were closed afterward. No frontend development server was started.

The packaged build and frontend checks pass with these corrections. The full
check is not green: it reaches the same three Rust test constructors requiring
the initial-color field, whose mechanical edits remain pending user permission.

Packaged live-output verification:

- Through the controller GUI, the walkthrough controller was changed to E1.31
  unicast at `127.0.0.1`, universe 1, with 512 channels. No physical receiver was
  involved and the project remains configured for loopback.
- A local UDP capture received 602 full data packets while the silent sequence
  played. Prop A's channel values varied; Prop B and unassigned channels remained
  zero. Full packets contained 512 lighting slots and the start code.
- Disabling live output emitted an all-zero full frame followed by three short
  stream-termination packets. The capture handles these separately from data
  frames; an earlier helper incorrectly assumed every packet contained slots.
- Capture metadata is in `target/authoring-live-capture.json`. This verifies the
  desktop network path, not physical fixture behavior or hardware timing.
- Live output was confirmed disabled, transport stopped at zero, and the app
  and preview were closed. No development server was started.

Fixture output replacement now uses one setup edit, with the same Add/Replace
choice as pixel output assignment. The language authoring layer recognizes
direct fixture-source / profile-encoder / destination routes, using the shared
route-edge validation and patch-pruning operations. Shared, grouped, or custom
processing requires an explicit patch edit. Destination validation runs inside
the desktop's candidate transaction, so a rejected replacement does not remove
the accepted assignment. Source imports, save scheduling, and history remain on
the existing setup edit path.

Packaged fixture walkthrough verified this path using the default one-channel
dimmer profile, created entirely through the GUI:

- Created `fixture_1` and a fixture element referencing it; source inspection
  confirmed the display document's explicit profile import.
- Assigned channel 181, replaced it with channel 200, then used Undo and Redo.
  Saved patch data returned to start slot 180 on Undo and 199 on Redo.
- Attempted replacement on occupied channel 1. The app reported the conflict,
  and saved patch data retained the channel-200 assignment.
- Added a mirrored assignment on channel 201 without removing channel 200.
- Saved, closed, and restarted the app. Setup retained the profile, fixture
  element, and both assignments, reporting Project checked and Saved.
- Live output remained disabled. The app and preview were closed afterward.
  This checks guided routing with a simple dimmer profile; complex profile
  functions and physical fixtures still need representative verification.

Typed fixture-control walkthrough:

- In the sequence inspector's Controls tab, added a clip targeting fixture node
  3, function 1, at 50% from 0 to 60 seconds. Saved source contains the explicit
  fixture-function target and constant normalized value `0.5`.
- During silent playback, loopback E1.31 capture received 1,093 full data frames.
  Channels 200 and 201 both held 128 in every captured frame, matching the
  runtime's rounded 8-bit conversion. Metadata is recorded in
  `target/authoring-control-capture.json`. This is network evidence, not physical
  fixture verification.
- Undo removed the clip from saved source; Redo restored its target, timing, and
  value. Output was disabled, transport stopped, and the app closed afterward.

Control timeline implementation now includes channel rows beneath their target
elements in the shared vertical layout and scroll bounds. Only channels with
clips create rows. Overlapping clips reuse the automation interval grouping and
slot assignment, and clip labels include value summaries and explicit cell
ranges. Selection opens the Controls inspector filtered to the selected clip.
Move/resize gestures commit through `UpsertControlClip` with the request captured
at gesture start, preserving target and value. Delete uses the existing control
edit; Escape or leaving the canvas cancels an in-progress control gesture.
Packaged manual verification confirmed that selecting the clip opens Controls,
right-edge resizing changed its duration, left-edge resizing changed its start
while keeping its end, and dragging the body changed its start while preserving
duration. Saved target node 3, function 1, and normalized value 0.5 remained
unchanged. Undo restored the previous start and Redo restored the moved start.
The app was closed with live output disabled. The visual check found a long row
label overflowing its gutter; row labels now use the existing label-fitting
helper. Inspector end times are rounded to avoid binary floating-point display
noise. Packaged recheck confirmed that the row label stays inside its gutter.
Escape during a drag preserved the saved sequence, and Delete followed by Undo
removed and restored the clip. Screenshot evidence is in
`target/authoring-control-timeline-final.png`.
Control values remain edited in the inspector; this increment does not add
curve-point gestures or multi-clip control selection.

After the final timeline edits, `cargo fmt` and the frontend type/lint/unused-code,
build, and 26 existing test checks passed. Full `pnpm check` again stopped at the
three existing Rust test constructors requiring the initial-color argument;
authorization to modify those tests remains pending. No tests were changed.

Reopening a moved control clip uncovered an inactive-fixture rendering error:
`Patch(Fixture(MissingFunction))` before the clip's start. Runtime fixture state
is cleared each frame and populated only by active controls/behaviors, but the
encoder required all functions to be present. The shared encoder now leaves
undriven channels at zero. Invalid prepared function indices, wrong value types,
missing indexed entries, and invalid output widths/slots remain errors.
Existing runtime tests, the runtime build without default features, and production
workspace Clippy passed; those existing tests do not cover this inactive-fixture
case. Full checks retain the same three test-constructor blocker.

Packaged verification of the inactive-fixture correction used a GUI-authored
clip starting at 15 seconds and lasting 5 seconds. A continuous loopback capture
received 1,797 full frames: both mirrored channels were zero before the clip,
128 during its five-second interval, and zero afterward. The two channels matched
in every packet. Capture metadata is in
`target/authoring-control-full-interval-capture.json`. Live output remained
streaming after the clip ended, with no MissingFunction error. An earlier capture
started too late to prove the pre-clip state; the full-interval capture supersedes
it. Stop intentionally terminates live output, so inactive rendering was checked
during playback. Output was disabled and the app closed after verification.

Desktop compiled-sequence export is available from the sequence toolbar. It
exports the currently open sequence and explicitly selected setup output ports,
in selection order. Export captures the checked immutable project session after
validating the originating GUI revision, uses the existing selected-output
compiler and portable encoder, and atomically saves through the native file
picker. Empty selections, stale requests, unavailable outputs, preparation
errors, and write errors are surfaced to the user. No dependencies were added.

Packaged manual export selected the walkthrough's 512-channel port and saved
`target/authoring-export.dawnseq` (2,622 bytes). Header inspection confirmed DAWN
magic, current format marker 6, a matching 2,606-byte payload length, and a matching
CRC32. Screenshot: `target/authoring-export-dialog.png`. This verifies the desktop
save path and container integrity, not firmware decoding or device compatibility.
The walkthrough's arbitrary 512-channel output is not proof of compatibility
with the ESP32 I2S loader's RGB/output limits. Device-aware limits, upload,
provisioning, persistent storage, and embedded transport are still outstanding.
The app and preview were closed after verification.

Frontend checks and production Rust Clippy passed for export. The packaged build
passed after its final display changes. Full checks still stop at the same three
test constructors awaiting permission for their mechanical initial-color update.

The existing ESP32 uploader now accepts ordinary desktop exports without a
benchmark checksum sidecar or a local firmware ELF. Frame verification is an
explicit `--checksums PATH` option, and image-hash evidence uses `--elf PATH`.
Rejection exercises require checksums so their post-rejection frame checks are
retained. Existing verification assertions were not modified. Documentation
commands explicitly request their verification inputs; ordinary uploads report
upload completion without claiming frame verification. Python compilation and
CLI help checks passed. No hardware upload was run for this change.

Desktop HTTP communication and firmware capability JSON require dependency
approval under AGENTS.md. Permission has been requested to use the existing
workspace reqwest client in the desktop crate and serde in firmware. Neither
dependency was added. This is separate from the pending three mechanical test
constructor updates. Firmware still needs device capabilities, transport,
persistent storage, and a first-class provisioning/upload UI.

Setup/controller diagnostics now use the existing closed-mapping validator for
setup objects, controller objects, E1.31/Art-Net protocol configuration, and
protocol-specific output ports. Multicast E1.31 does not accept an unused unicast
destination, and E1.31 ports reject Art-Net-specific address fields.

All existing project-IO tests passed. In a separate manual project copy under
`target/authoring-controller-diagnostic-probe`, the normal CLI reported the exact
file and line for `priorityy` and for an invalid E1.31 `port_address` field.
The unchanged walkthrough project passed CLI analysis with six compiled documents.
No tests were added or modified. Unknown-field coverage for other setup-related
objects remains to be completed.

For this increment, `cargo fmt` passed. `pnpm check` passed frontend typechecking,
lint, unused-code analysis, production build, and all 26 existing frontend tests,
then stopped at the same three `initial_color` test-constructor errors.
`cargo clippy --workspace --lib --bins --all-features -- -D warnings` passed.
No tests were added or modified.

Prop definitions, geometry, preview layouts, prop instances, transforms, and
bindings now reject unknown fields using the shared mapping validator. Point,
rotation, and scale parsing retain the actual document path instead of reporting
`<inline>`. A separate manual copy under
`target/authoring-geometry-diagnostic-probe` produced exact file-and-line errors
for `point_cout`, a nonnumeric position coordinate, and `propz`. The original
walkthrough still passed CLI analysis with six compiled documents.

All 49 existing project-IO integration tests and project-IO library Clippy passed.
`cargo fmt` passed; `pnpm check` passed frontend checks and all 26 frontend tests,
then stopped at the same three missing `initial_color` test-constructor fields.
No tests were added or modified. Broader authoring and embedded-device work remain
open; these diagnostics do not constitute complete acceptance of the goal.


The user subsequently authorized test updates and dependencies. The three
`ChangeEffectDefinition` test requests now supply the existing fixture layer
color; their behavioral assertions are unchanged. Dependency approval is no
longer blocking device communication work.

New Project now creates an empty element tree, preview layout, and patch, with no
controller or prop definition. The initial silent sequence remains available.
The template regression test now verifies those empty authoring resources rather
than requiring the previous starter controller. The earlier GUI walkthrough
remains evidence for editing a starter project; acceptance from this new empty
project still needs its own complete walkthrough.

After the authorized test updates and empty-template change, cargo fmt and the complete pnpm check gate passed, including all workspace tests and all-target Clippy. No dependency manifest changes have been made yet. Upstream documentation confirms reqwest 0.13.4 and serde 1.0.229 match the existing workspace versions for the planned device communication work.


Firmware now exposes authenticated `GET /capabilities` through picoserve JSON.
Its format marker comes from the runtime codec, and its admission/output limits
come from the constants used by firmware. It explicitly reports volatile storage
and distinguishes I2S output from evaluation-only builds. Serde 1.0.229 was added
with default features disabled, and picoserve's JSON feature brings its required
serde-json-core 0.6.0 transitive dependency. The desktop HTTP client is still to be
connected to a device workflow.

The Python uploader validates the archive header before provisioning, then checks
the device's format and maximum payload size before uploading. It records the
capability response without logging the token. Python compilation/help and manual
invalid-header/length invocations passed. Both I2S and loader-only Xtensa release
checks passed with the lockfile. No board was flashed or contacted, so the new
HTTP endpoint and its serialized response still require live validation. Device
transport, persistent storage, and desktop provisioning/upload remain unfinished.

For this increment, cargo fmt and the complete pnpm check gate passed.


Desktop sequence export now includes capability checks and upload to a provisioned
device. The approved reqwest workspace dependency is used with bounded responses,
connection/request timeouts, no redirects, and a sensitive token header. The
selected immutable sequence is compiled through the existing export path. Upload
requeries capabilities and rejects incompatible formats, payload sizes, output
counts, or channel widths before PUT. The dialog states that upload replaces the
running sequence and starts playback; it clears the token when closed.

An authorized desktop HTTP regression test uses a local server to check an
accepted 90-channel output and rejection of a 512-channel port before upload.
All 35 desktop tests and the full pnpm check gate passed. Manual native UI
inspection confirmed an actionable invalid-address error and token clearing on
close/reopen. A visual inspection identified unstyled fields; these were moved
onto shared dialog form/button styles. No physical device was contacted. This is
upload for an already provisioned device, not completion of USB provisioning,
persistent storage, or device transport controls.

The final styled dialog was rebuilt and visually verified in the packaged app (target/authoring-device-upload-dialog-final.png). The app was closed after inspection. The final styling changes passed frontend typechecking and production packaging; the preceding full gate passed all tests and Clippy.


Empty-project acceptance now has a desktop command-path regression test. Starting
from New Project's generated files, it adds two 30-pixel props, a 180-channel
controller, and disjoint routes; verifies controller undo/redo; rejects overlapping
routing without replacing the accepted Arc snapshot; adds a built-in Pulse;
renders 60 frames with only Prop A illuminated; and saves/reloads exact typed
project equality. The focused test and full pnpm check passed (36 desktop tests).

A separate native UI walkthrough created `target/authoring-empty-walkthrough`
through File > New Project, starting with zero elements/controllers. Visible forms
created Prop A and Prop B with 30 pixels each, a unicast loopback E1.31 controller,
and channels 1-90/91-180. The timeline menu added Pulse to Prop A. Silent playback
advanced, and live output reached streaming. A loopback capture recorded 474
180-channel data frames, including 48 nonzero Prop A frames across 49 distinct
summed levels; Prop B remained zero. Evidence is in
`target/authoring-empty-live-capture.json`. No physical lights were measured.

One save reported Windows access denied while the diagnostic workflow was reading
the patch file. Save All subsequently succeeded and the closed project passed CLI
analysis (five compiled documents). The relationship to the simultaneous read is
not proven; do not treat that transient save issue as diagnosed or fixed.

The project reopened in the packaged UI with its saved sequence. Seeking into the Pulse produced 30 illuminated preview pixels; target/authoring-empty-preview-reopened.png records the paused preview. The app was closed after verification. This completes the basic empty-project authoring/playback/save-reopen walkthrough, with loopback rather than physical output evidence. The separate transient save error remains an investigation item.


The contradictory save indicator is fixed: it previously read only the active
buffer, while GUI setup edits can change an unopened display or patch document.
Snapshots now derive pending save metadata from all working documents, and the
status bar uses that list for failures, conflicts, and unsaved changes. The tooltip
identifies affected paths and failure details. No second save-state owner or
automatic retry path was added.

The Windows acceptance test reproduced failed atomic replacement by holding the
display file open without delete sharing, while the active setup file remained
clean. It verified a pending failed save for the display file, released the handle,
retried Save All, and verified that all pending saves cleared and typed data
reloaded exactly. The focused test passed. This demonstrates the file-sharing
failure mechanism; it does not identify which handle caused the original manual
walkthrough failure.

For the project-wide save indicator change, cargo fmt, regenerated bindings, and the complete pnpm check gate passed, including the Windows file-sharing regression. No desktop app or dev server was started for this increment.


Desktop export now offers USB Wi-Fi provisioning using the approved serialport
4.10.0 dependency (current upstream release checked before adding it). The form
lists serial devices, explicitly describes the reset, accepts WPA2 credentials,
and fills the returned address/token before checking capabilities. It uses the
firmware's existing P/W protocol rather than invoking Python. Replies preserve
partial lines across read timeouts, enforce bounded line sizes and phase deadlines,
and validate tokens and addresses without echoing secret serial contents.

The packaged UI was visually checked in
`target/authoring-usb-provision-dialog.png`. Read-only discovery listed the COM4
CP210x bridge. A dummy password was entered, then closing/reopening the dialog
confirmed both password and token were empty. No serial port was opened and no
board was reset or provisioned. Hardware Wi-Fi acceptance is still outstanding.
The form was closed and the test app exited. Firmware installation, persistent
credentials/sequences, and device transport remain separate unfinished work.

Validation: pnpm check passed frontend checks and all workspace tests (38 desktop tests), then reported one collapsible-if Clippy warning. After the equivalent condition was simplified, cargo fmt and all-target, all-feature workspace Clippy passed. The serial transcript tests cover fragmented replies, UTF-8 byte lengths, malformed tokens, and credential bounds. Physical provisioning remains unverified.

Device playback now has authenticated status, play, pause, and stop endpoints in
output-enabled firmware, with matching controls in the desktop export dialog.
Pause holds the sampled position, play resumes, and stop rewinds and renders black
frames. Each uploaded sequence owns its cursor, fixing replacement uploads that
previously inherited the render task's old frame index. Frame count is calculated
once during loading. The existing playback mutex owns both commands and rendering.
Status reports the latest prepared frame and requested mode; queued DMA frames
can precede the command's visible effect. The UI labels position as a snapshot
and offers explicit refresh, clearing uncertain status after failed requests.

Validation: cargo fmt, regenerated bindings, and the complete pnpm check gate
passed, including 39 desktop tests. A local HTTP server test verifies authenticated
GET/status and bodyless POST/control requests and their returned states. Two
standalone firmware host tests cover pause/resume, stop/restart, looping, single
frame sequences, and fresh upload cursors. Both loader-only and I2S-output firmware
passed locked release checks. No desktop app, dev server, or hardware session was
started for this increment; physical transport timing and blackout remain
unverified. Persistent storage, firmware installation, and the broader authoring
workflows remain unfinished.

Controller installation now has a reproducible Windows image build command:
`./firmware/esp32/build-image.ps1`. It builds the locked I2S-output loader and uses
espflash to package the bootloader, partition table, and application. The single
CSV layout reserves the final 256 KiB of a 4 MB board for Dawn data. Packaging
omits flash-size padding and checks its output against the data partition boundary
before replacing the finished image. Existing loader ELF flashing instructions
now use the same table. The desktop installer and use of the reserved storage
are still unfinished; this image continues to report volatile sequence storage.

The release firmware linked successfully. The generated image was 1,001,472 bytes
with SHA256 `BBE81E9835AAD659C67AEE3B388220F4C38BC244937ED51EEF07443856F890E9`.
Read-only verification compared its embedded partition table to espflash's CSV
conversion, checked bootloader/application headers and partition boundaries, and
confirmed that the image ends before data at `0x3c0000`. PowerShell parsing and
root cargo fmt checks passed. This increment changed packaging, the partition
table, and documentation only; the desktop gate was not repeated. No serial port
was opened or board flashed. The first packaging attempt exposed an espflash
panic for an unrecognized data subtype; the final table uses its supported
generic `undefined` data subtype.

Persistent device storage is now connected. The new no_std
`crates/dawn-device-storage` uses littlefs2 0.8.1 for bounded records with CRC32
payload checks, closed temporary writes, and atomic replacement. Blank partitions
are initialized; corrupt or incompatible data is reported without automatic
erasure. Credentials use typed JSON serialization with explicit string unescaping.
A regression reproduced incorrectly restored escaped passwords before the decoder
was corrected. Credentials are saved after Wi-Fi connects, and the token survives
ordinary resets. The controller gives USB reprovisioning a three-second boot
window and otherwise starts with saved credentials. Saved playback starts before
waiting for DHCP. Uploads validate, persist, then replace running playback.

The firmware adapter checks the actual partition table, bounds all access to the
Dawn partition, uses aligned buffers, and enables the SDK's multicore flash
parking. The desktop reports persistent storage and surfaces explicit storage
errors received during provisioning. There is still no desktop storage reset or
repair flow, and firmware installation is not integrated into the desktop.

Validation: cargo fmt, regenerated bindings, and the complete pnpm check gate
passed on the final code, including 40 desktop tests and five storage tests.
Storage coverage includes remounts, bounded reads, interrupted writes/erases,
complete old-or-new recovery, retained credentials, payload corruption, escaped
credentials, and refusal to format damaged storage. Host checks used the installed
host libclang; the environment's ESP cross libclang cannot build Windows bindings.
The README now documents that host prerequisite. The loader-only firmware check
passed, and the output-enabled release linked and packaged successfully.

The final image is 1,052,800 bytes, SHA256
`4A54AF73B9C19F19CBE2DFE0AC4AE61D4FCEA44FE5D3E584C584E5E80975BE01`.
The C filesystem requires Xtensa long calls; tinyrlibc 0.5.1 supplies only the two
missing string functions, avoiding full newlib runtime requirements. New direct
dependency releases were checked before adding them; esp-storage remains on the
coordinated existing SDK revision. No board was flashed, reset, or provisioned.
Physical power-loss recovery, offline restart, heap headroom, flash-write timing,
and output behavior remain unverified. No app or dev server was left running.

Desktop storage recovery is now available in the USB section of sequence export.
The operator must select a device and check an explicit acknowledgement before
the erase button enables. Changing or refreshing the selection, provisioning,
closing the dialog, and starting an erase clear that acknowledgement. The desktop
uses a separate R/ready/F/complete serial exchange and never sends an erase command
from ordinary provisioning or upload. It reports failed or missing completion
without claiming that saved data remains intact.

The firmware separates partition validation from filesystem mounting so damaged
storage can enter recovery. Erasure only accesses the validated Dawn partition;
invalid partition layouts report recovery unavailable. After erasing, firmware
flushes the completion reply and resets. No implicit erase was added to normal
boot, loading, or provisioning.

Validation: cargo fmt and the full pnpm check gate passed, with 42 desktop tests
and six device-storage tests. Focused erase transcript tests passed again after
the final uncertainty-message adjustment. Both firmware variants checked and the
output-enabled release image built: 1,055,920 bytes, SHA256
`36C67E8249EDD215C4421520D20578B54D5DFB0855A021F6D1E27B95872A62CE`.
Tests cover explicit recovery from damaged metadata, deletion of both records,
unavailable recovery, and failed erasure.

The packaged desktop was visually checked in
`target/authoring-device-recovery.png`. With COM4 selected, Erase remained disabled
until confirmation was checked; refreshing cleared confirmation and disabled it
again. The erase button was never pressed. No serial port was opened or hardware
reset/erase performed, and the app was closed. Physical recovery acceptance and
desktop firmware installation remain outstanding.

Desktop firmware installation now embeds the output-enabled controller image and
uses espflash 4.5.0 directly. The USB form requires a selected port and explicit
firmware-replacement confirmation, displays connection/write/verification/restart
progress through a typed Tauri channel, and clears confirmations when the selected
port changes or the list is refreshed. Installation and provisioning remain
separate explicit actions. No external flashing utility or ESP development
toolchain is required by the desktop installer.

Before opening USB, installation checks the bundled SHA256 and the current
partition layout. Before flash writes, it requires the supported ESP32 chip,
4 MB flash, 40 MHz crystal, dual-core/240 MHz configuration, and disabled secure
boot/flash encryption. The image ends before the Dawn data partition. espflash
verifies the written image before restarting. Failed or interrupted installation
reports that the image may be incomplete and instructs the user to retry.

The image build script regenerates both the developer output and committed
`apps/desktop/assets/firmware` image/checksum. Current embedded image: 1,055,920
bytes, SHA256 `36C67E8249EDD215C4421520D20578B54D5DFB0855A021F6D1E27B95872A62CE`.
The full `cargo fmt` / `pnpm check` gate passed, including 44 desktop tests.
Installer tests cover checksum corruption, mismatched partition tables, and
incompatible chip/flash/crystal configurations. This is host verification;
physical installation, provisioning, power-cycle persistence, recovery, and
output timing still require hardware acceptance. The broader authoring goal
remains active.

The packaged desktop build passed and its installer form was reviewed in
`target/authoring-device-installer.png`. With COM4 selected, installation stayed
disabled until replacement confirmation was checked; refreshing devices cleared
confirmation and disabled installation again. The Install button was never
pressed, no serial connection was opened, and the review app was closed.

Controller membership now offers **Create editable controller copy**. It replaces
that setup's controller reference with a project-owned definition and makes an
independent copy of its patch, retargeting matching sinks without changing the
original controller or patch. Port settings, assignments, other sinks, node IDs,
and graph edges are retained. Other setups can keep using the original objects.
The same explicit operation is available for shared local controllers and
read-only dependency controllers; the containing setup must be project-owned.
The source owner registers new identities and the shared import path establishes
all element/controller/fixture references needed by the copied patch.

The blank-project acceptance flow now copies its assigned controller, checks that
the original objects remain unchanged and all copied sinks use the new identity,
undoes/redoes the complete session, renders the two-prop sequence, and verifies
save/reload equality. This acceptance test passed. It does not yet exercise a
separate dependency package or the new button in a running UI. Editable copies of
whole dependency-owned element/preview structures remain outstanding.

Controller-copy validation: cargo fmt and the full pnpm check gate passed. No desktop app or development server was started for this increment.

Editable layout copying now creates project-owned element trees, preview layouts,
patches, preview shape definitions, and fixture profiles in one GUI transaction.
Numeric element/placement/patch IDs and sharing within the copied layout are
preserved. The original objects stay available and unchanged. For the active
root setup, matching effect and typed-control selections in project-owned root
sequences are retargeted to the new tree. A dependency-owned sequence requiring
retargeting blocks the operation before source registration; copying imported
sequences remains separate unfinished work. Controllers retain their explicit
copy action. Copy controls expand for dependency-owned layouts and are collapsed
for ordinary editable layouts.

A dependency acceptance fixture uses the starter's 30-output layout, patch,
controller, and shared vertical prop as a separate path package. Its same-named
`setups/main.setup.dawn` exposed an existing GUI request bug: resolution compared
module-relative paths globally, so a dependency could make a project document
ambiguous. GUI resolution now uses SourceProject's workspace-path/module mapping,
the same owner used elsewhere for path dependencies.

Validation: cargo fmt and the full pnpm check gate passed, including 45 desktop
tests. The blank-project test copies a layout after creating its effect, verifies
clip retargeting and unchanged original objects, performs undo/redo, renders the
expected two-prop output, and saves/reloads typed equality. The dependency test
copies its controller and layout, preserves the single shared prop definition
within all 30 copied placements, confirms project ownership and dependency GUI
read-only behavior, edits a copied element, undoes/redoes both copy operations,
and saves/reloads without changing any of the four dependency source files.
No packaged UI or physical hardware was exercised for this increment. Dedicated
fixture/control-copy cases and imported-sequence copying remain outstanding.

Fixture/control-copy acceptance exposed a missed fixture identity in patch source
value types: the element and encoder referenced the copied profile while the
source still referenced the original, so graph validation rejected the edit.
The same identity was omitted by document-path refactoring. PatchNode now exposes
its fixture-profile reference for sources and encoders, and copying, import
registration, and document moves use that shared accessor.

The new fixture acceptance flow authors two fixtures sharing a profile, a group
16-bit dimmer clip, and an indexed shutter clip through GUI commands. It copies
the layout, checks shared profile identity and control targets, compares exact
output bytes before/after copying (including blackout outside clip lifetimes),
undoes/redoes, saves/reloads, then moves the setup document and renders again.
At the active frame both versions emit [128, 0, 32, 128, 0, 0]. The focused test
passed after reproducing the original graph mismatch.

A user-facing walkthrough is now in docs/first_show.md and linked from README.
It covers an empty project, two 30-pixel props, controller/channel assignment,
Pulse, silent preview playback, saving/reopening, and common blocked operations.
Its labels were checked against the current UI source; this increment did not
repeat the packaged manual walkthrough or exercise hardware.

Validation for the fixture-reference fix: cargo fmt and full pnpm check passed, including 46 desktop tests. Imported-sequence copying and physical controller acceptance remain open.

Imported sequence handling now has an editable-project export foundation in
`dawn_project_io::export_editable_project`. It stages a separate, new project,
relocates every loaded dependency document into project ownership, rewrites
resolved YAML and effect-DSL imports as local document imports, copies referenced
audio, and writes a dependency-free manifest and lockfile. The staged project
must load successfully before the destination is published. Existing destinations
are rejected and a failed copy leaves no partial destination. The original
session and dependency files remain unchanged. This is an IO operation; the
new desktop command/dialog is still pending.

Document moves and editable export share source import rewriting. Local DSL
imports retain their surrounding source text; dependency import expressions are
replaced with the resolved local document list. Paths use forward slashes in
authored data on Windows. The IO test uses a starter sequence with a custom effect,
a nested package import, and audio, verifies all exported objects are project-owned
and dependency imports are gone, compares audio bytes and rendered controller
frames, and checks existing-destination rejection and missing-audio cleanup.
The test uses the existing internal dawn-elaboration crate as a dev dependency.

A separate UI wiring correction restores Undo and Redo to the Edit menu, which
had incorrectly contained Save. Existing command handlers and shortcuts are reused.

Validation: cargo fmt and full pnpm check passed for editable-project export, source import rewriting, and the Edit menu correction. No desktop application or hardware was started in this increment.

The desktop now exposes **File ? Create Editable Project Copy...**, sharing the
new-project destination dialog and workspace transition flow. The copy opens its
setup in GUI mode. Save All persists the current edits before copying; Discard
loads the saved source without dropping the current draft until copying succeeds;
Cancel creates nothing. Invalid or occupied destinations leave the original
project and draft open. Folder fields and dismissal are disabled during creation,
and failures appear inside the dialog. Workspace transitions run on the blocking
worker pool so file copying does not block the native UI thread.

Desktop acceptance covers the Save All, Discard, and Cancel decisions, occupied
folder rejection, preserved draft identity on failure, reopening the copy in GUI
mode, and exact typed projects on both source and destination disks. The first-show
guide and README now point users to the editable-copy command.
Validation: cargo fmt and the full pnpm check passed, including 47 desktop tests. The packaged debug build also passed; it retains the existing frontend bundle-size warning.

Packaged UI acceptance created `target/authoring-project-copy-walkthrough` from
the two-prop walkthrough using File, the new dialog, and the native folder
chooser. Dawn opened the copy's setup in GUI mode with both 30-pixel props, the
controller, and the 1–90/91–180 assignments intact, reporting Project checked and
Saved. The dialog was visually reviewed in `target/authoring-project-copy-dialog.png`.
Save/Discard/Cancel and failure preservation were exercised by the desktop test;
this manual pass used an already-saved project. No physical device was contacted.

Guided dimmer and indexed-control output assignment now builds typed scalar or
indexed sources, the existing component conversion/mapping filter, and 8-bit
channel quantization. Each cell occupies one channel on a selected output.
Indexed mappings require exactly one explicit 0–255 value per defined option;
missing, extra, and duplicate option identifiers are rejected. The setup form
shows channel requirements and controller usage and supports replacement or
additional mirrored routes.

Fixture and control routes share destination capacity/overlap validation and
linear-route insertion. Control replacement recognizes complete direct routes
and rejects group, partial, shared, or custom processing. Source references are
registered through project IO and edits use the normal DesktopState transaction.
The new acceptance test covers exact scalar/indexed output, mirrored assignments,
replacement, rejected overlap/overflow/incomplete mappings, undo/redo, and typed
save/reload. Control resizing and physical output validation remain separate work.
Validation: cargo fmt and full pnpm check passed, including 48 desktop tests and exact dimmer/indexed controller-frame assertions. The control editor help now points to the guided assignment forms.

The packaged manual pass added a scalar control and an indexed control to
`target/authoring-project-copy-walkthrough`, then assigned them to channels 1 and
2 of a second controller. Blank indexed values blocked submission without
changing the patch file; explicit Off=0 and On=200 values produced the saved
indexed mapping and 8-bit route. No live output was enabled. The visual review
also found setup controls using browser-default input/button styling; setup forms
now reuse the existing themed form and button CSS rules, including disabled
opacity, and indexed mapping labels use the existing field layout.

Final validation passed cargo fmt and full pnpm check after the CSS/help changes
(`target/authoring-control-output-final-check.log`), followed by a successful
packaged build. The reopened setup retained both new assignments. The themed
form was visually reviewed in `target/authoring-control-output-styled.png`; the
app was then closed. Physical control output was not exercised.

Changing the cell count of a scalar or indexed control now resizes all complete
guided assignments in the same DesktopState candidate. Source/filter widths and
sink channel counts change in place, retaining patch node IDs, edges, indexed
values, destinations, and start channels. Resizing and assignment share direct
route recognition and destination validation. Partial, group, shared, or custom
routes are rejected; another patch referencing the control produces an error
naming that patch. The GUI checks patch ownership when routes actually change.

The control-output acceptance flow now shrinks and grows both control kinds,
including mirrored indexed routes, checks exact output bytes and stable patch
identifiers/edges, exercises undo/redo, and verifies collision/overflow/zero-cell
rejection leaves the accepted Arc unchanged. Save/reload still compares the full
typed project. The first-show guide and control editor explain resizing.

A related follow-up remains: inspect option-ID edits after indexed assignment for
stale mapping coverage. The new guided assignment requires complete mappings,
but editing the element definition uses a separate existing path.
Validation: cargo fmt and the full pnpm check passed for control resizing, including 48 desktop tests (`target/authoring-control-resize-check.log`). This increment used DesktopState-to-render-to-save/reload acceptance; no packaged app, development server, or physical device was started.

Indexed mapping follow-up: runtime already returns MissingIndexedMapping rather
than substituting a value. The missing check was earlier authoring validation:
adding an option could leave a project that failed only when that option played.
Canonical setup validation now checks indexed mappings against all options of
selected indexed elements, including group selections, and requires ID 0 for
inactive cells. Errors identify the mapping node and missing option. The runtime
error remains as an invariant safeguard. Extra entries are permitted in authored
patches so a mapping can be prepared before a new option is added.

Guided assignment requires values for every option plus inactive ID 0. When the
visible options omit zero, the form includes a separate required inactive value;
no default channel value is invented. Acceptance rejects a newly added unmapped
option without replacing the current Arc, checks missing active/inactive mapping
diagnostics, and exports/reloads a control whose visible options omit zero while
retaining its explicit inactive mapping, with exact idle and active output bytes.

Validation passed cargo fmt and full pnpm check, including 48 desktop tests
(`target/authoring-indexed-mapping-check.log`), and the packaged debug build.
In the manual walkthrough project, removing visible option ID 0 retained the
valid existing inactive mapping. Selecting that control in the assignment form
showed the separate inactive field; UI Automation reported IsRequiredForForm=true.
The form was visually reviewed in `target/authoring-indexed-inactive.png` and the
app was closed. No live output or physical device was exercised.

Guided color-prop authoring now carries the existing ColorCapability through
creation, geometry/pixel resizing, duplication, projection, and routing. The light
form reuses the patch editor's capability form for RGB, RGBW, and discrete emitters.
The desktop DTO exposes structured capability and the canonical component count;
output assignment supports RGB/GRB, RGBW/GRBW, or declared discrete-emitter order.
The domain route planner accepts a complete component permutation and uses the
canonical component count for port capacity, channel spans, route recognition,
resizing, and quantization widths. Pixels stay whole when spanning ports.

Discrete color matching remains exact. Canonical setup validation rejects a
routed discrete filter without an inactive black mapping; the shared capability
form explains inactive colors and additional colors introduced by fades. No
runtime interpolation or missing-color substitution was added.

The new color-prop acceptance scenario creates RGBW and discrete props, assigns
multiple ports and a discrete permutation, grows RGBW across ports, checks failed
growth and invalid permutations preserve the accepted Arc, duplicates the RGBW
prop without routes, authors constant-color effects through GUI edits, checks
exact controller bytes and idle output, and saves/reloads the full typed project.
Static test colors come from styles.css. The original RGB acceptance remains.

Validation passed cargo fmt and full pnpm check, including 49 desktop tests
(`target/authoring-color-props-check.log`), and the packaged debug build. The
manual walkthrough created a two-pixel RGBW light, selected GRBW order, and saved
its eight-channel assignment at channels 3–10 on controller 2. The UI showed four
channels per pixel and updated occupied/free ranges; saved patch data retained
RGBW conversion, four-component order, and the eight-channel sink. Visual evidence:
`target/authoring-rgbw-assignment.png`. The app was closed without enabling live
output. Discrete routing and port-spanning behavior were verified by the automated
acceptance test; no physical lighting was exercised.

## Setup source field diagnostics

Setup source loading now rejects unknown fields in element trees and nodes,
indexed options, color capabilities and discrete mappings, patch sources,
selections and cell ranges, filters and indexed mappings, sinks and edges, and
fixture profiles, functions, channels, behaviors, indexed entries and dimming
curves. Each parser uses the existing allowed-field diagnostic helper and its
current variant's fields. Previously these mappings silently ignored additional
keys, so a typo could survive loading and disappear during a typed save.

Acceptance coverage checks unsaved starter layout/patch edits and a temporary
fixture project. Unknown fields reject the candidate, identify the owning
document and exact value range, and leave the saved project unchanged. Existing
round-trip and desktop authoring checks exercise valid serialized variants.

Validation: cargo fmt and full pnpm check passed, including all 19 IO diagnostics
and 49 desktop tests. Evidence: target/authoring-setup-diagnostics-check.log.
This increment changes source diagnostics only; no manual UI or physical output
acceptance was performed.

## Editing color capabilities on existing lights

Each color light now exposes Edit color capability, including lights without a
single guided preview placement. The form reuses the capability editor, requires
an output order, blocks concurrent GUI edits, and shows rejected-edit errors.
The selected capability and order apply to every guided route for the light.

The typed authoring operation reads and validates the old routes before changing
the element, then uses the existing route planner to rewrite them together.
Starting destinations are retained and available node IDs are reused. Shared,
custom, partial, or other-patch routes require explicit edits; output collisions
and capacity failures reject the DesktopState candidate. No extra project clone
or routing model was introduced. Output-order validation is shared with assignment.

The color-prop acceptance scenario now changes RGBW to RGB and back, checks
channel widths and undo/redo, rejects insufficient capacity and malformed orders
without replacing the accepted Arc, then edits a discrete color mapping and
checks its changed controller bytes and save/reopen behavior.

Validation: cargo fmt and full pnpm check passed (49 desktop tests), followed by
the packaged debug build. Logs: target/authoring-color-edit-check.log and
target/authoring-color-edit-build.log. The manual walkthrough changed the existing
two-pixel RGBW light to RGB/GRB. Its saved assignment stayed at start_slot 2 and
shrank from eight to six channels; the UI projected RGB and showed the required
output-order selector. Screenshot: target/authoring-color-edit.png. The app was
closed afterward. No live output or physical device was activated.

## Direct controller channel verification

Setup Patching now includes an explicit, ten-second controller channel test.
The user selects a controller, port, contiguous channel range and byte value.
Only that port is opened; its unselected channels are zero. This bypasses patch
and sequence authoring and does not alter typed project state or preview.
The request resolves the current setup with the existing GUI identity path,
checks the project revision and validates membership and range before opening
transports. A Stop output action remains available while sending.

Tests use the existing live-output worker and transports, not a second sender.
The worker owns exclusive sequence/test output and terminates with blackout on
stop, expiration, shutdown, or suspension for project changes. Testing is an
explicit snapshot state and never participates in automatic sequence-output
resume. The output poll reports test state/errors to the setup form; sequence
transport also recognizes testing as active output that can be stopped.

Validation: cargo fmt and full pnpm check passed, including 50 desktop tests
(target/authoring-output-test-check.log), and the packaged debug build passed.
The new loopback Art-Net acceptance test verifies exact bytes, the selected port,
explicit stop, suspension without resume, timeout blackout, malformed/stale
request rejection, and unchanged accepted project state.

In the packaged UI walkthrough, controller_1 was verified as E1.31 unicast to
127.0.0.1, then the new form sent channel 1 at value 32. The localhost capture
received 487 matching frames and a final blackout, with zero unexpected values
(target/authoring-output-test-capture.json). The form displayed Stop output and
testing status, then returned to idle automatically. The reviewed screenshot
(target/authoring-output-test.png) shows the idle form after timeout. The app and
capture process were closed; no physical lighting or multicast output was used.

## Output state on rejected starts and suspension

A rejected sequence-output start previously changed only the workspace's output
snapshot to Error. The service and worker could still be sending a channel test,
and their next poll would overwrite that error state. Start validation now returns
an explicit command error through the existing frontend Result wrapper, retaining
the authoritative output state and its available Stop action. An invalid request
does not replace the active sender or claim that it stopped.

Suspension now drains pending worker updates before deciding whether sequence
output should resume after preparation. A terminal failure or automatic stop
that occurred between UI polls cannot be mistaken for the old Preparing/Holding
state. A queued Holding update still preserves normal resume behavior.

Acceptance extends the loopback channel test to reject sequence start while no
sequence is prepared, verify both snapshots remain Testing, and receive the next
unchanged packet. A focused service test covers queued Error, Disabled, and
Holding updates at suspension.

Validation: cargo fmt and full pnpm check passed, including the loopback and
queued-worker-state regressions (target/authoring-output-state-check.log).
No manual app or physical-output run was needed for this state/command change.
Remaining output diagnostic gap found during this review: terminate_active still
discards blackout_and_terminate failures, so stop error reporting is unverified.

## Acknowledged output stops and termination diagnostics

Output now reports Stopping until its worker acknowledges the stop. The output
poll remains active through that acknowledgement, and setup/sequence controls
prevent another start while stopping. Already-disabled output stays disabled.
Normal stop, playback end, test timeout, replacement, worker send/render failure,
and worker shutdown share the stop-result path. Blackout and protocol termination
are both attempted, including when blackout fails; every controller is visited.

A failed stop releases the transports but reports Error with the affected
controller and connection guidance. If rendering/sending also failed, that error
is retained alongside the termination failure. Repeated stop requests retain the
failure; a new explicit start begins a new attempt. Shutdown retains its worker
result instead of overwriting it with an unconditional disabled snapshot.

Automatic resume requires both successful preparation and an acknowledged stop.
The existing output poll completes a deferred resume; failed stops cancel it.
This avoids losing a stop error when preparation finishes before the worker does.

Acceptance includes a forced Art-Net address codec failure before any network
send, error retention across repeated stops, original-plus-termination messages,
queued stop-success/failure resume decisions, and the real loopback stop handshake.

Validation: cargo fmt and full pnpm check passed, including the forced-stop-error,
resume ordering, and loopback handshake regressions. Evidence:
target/authoring-output-stop-check.log. No physical controller was exercised and
no app or frontend server was left running in this increment.

## Sequence transport navigation ordering

Completion review found independently dispatched unload/load commands in
EditorPane, plus a loaded-audio key recorded before the native load completed.
That could let cleanup unload a newer selection or let a stale load suppress a
later retry. Silent sequences also shared keys across different project epochs.

SequenceAudioSync now owns ordered native audio transitions across editor effects
and remounts. Superseded queued targets are skipped, replacing a sequence uses the
native load operation directly, and only an accepted response records the loaded
key. Failed or stale operations leave the native state unknown so a subsequent
load or cleanup is not skipped. Errors still reach runSnapshotCommand's visible
error state and the synchronizer's caller. An unrelated edit with the same audio
key does not restart playback. Keys include project epoch, and pending projections
do not select audio using the previous displayed sequence's metadata.

Controlled asynchronous tests cover overlapping navigation and cleanup, failed
and stale loads, subsequent cleanup/retry, same-key playback preservation, and
same-named silent sequences in separate projects.

Validation: cargo fmt and full pnpm check passed with the final projection and
identity changes, including the three new frontend audio-transition tests
(target/authoring-audio-order-check.log). This increment used controlled async
acceptance rather than a manual native-audio race reproduction. No app or frontend
server was left running. Overall completion remains unproven, particularly the
physical embedded install/provision/power-cycle/output workflow.

## Sequence playback identity and editable dependency copies

Playback and clip-raster sequence lookup reconstructed a project-module identity
from a workspace path, duplicating the shared GUI resolver and selecting the
first sequence when no object key was supplied. resolve_sequence_id now delegates
to gui::resolve_request, requires a sequence view and current revision, and checks
the resulting typed sequence. Module identity and ambiguity handling remain in
one place. This does not change dependency editing permissions.

The initial acceptance assumption was wrong: dependency documents intentionally
return a read-only blocked GUI, not a sequence projection. The supported user
workflow is Create Editable Project Copy. Acceptance therefore checks canonical
identity resolution and the read-only boundary first, exports an editable copy,
opens the copied sequence, loads its duration, plays/pauses and renders a frame.
It checks the prepared target is the copied setup/sequence, the original project
is unchanged, playback does not replace the accepted copied-project Arc, and
wrong-view or stale identity requests fail.

The blocked dependency GUI now names File > Create Editable Project Copy as the
next action instead of only saying read-only. The same message is used for the
panel reason and its diagnostic.

Validation: cargo fmt and full pnpm check passed, including 53 desktop tests
(target/authoring-imported-playback-check.log). The new acceptance exercises native
silent transport and render preparation without activating network output. No
manual GUI or physical hardware run was performed for this increment.

## Fixture color and behavior acceptance

Added native GUI-command acceptance for RGB and RGBW fixture profiles. Each
scenario authors color-mixing channels, dimmer/shutter/color-wheel/prism functions
and behaviors, assigns a controller output, and creates a constant white effect.
Expected bytes verify RGB/RGBW encoding and automatic behavior values. A bounded
set of typed controls overrides every function, including a black color override;
after those clips end, automatic behavior resumes. The idle frame is black.

The same scenario rejects a duplicate behavior without changing the accepted
Arc, exercises undo/redo, saves/reloads the complete typed project and compares
frames before, during and after the overrides. Test palette values come from
styles.css. No runtime or editor fix was needed by the focused scenario. This is
native GUI-command acceptance, not a manual walkthrough of every frontend field
or a physical fixture test.

Validation: cargo fmt and full pnpm check passed, including 54 desktop tests
(target/authoring-fixture-colors-check.log). The acceptance review now records
native fixture color/behavior coverage and retains the separate manual-form and
physical-fixture verification limits. No application or device was started.

### Advanced scalar patch acceptance

Added native GUI-command acceptance for a custom scalar patch with a selected
cell range, scalar-to-components conversion, gamma/custom dimming curves,
scale/invert, fan-out, 8-bit quantization and both 16-bit byte orders. Exact
controller bytes are checked before and after save/reload. A mismatched width
rejects the candidate without changing the accepted session; a corrected draft
applies, projects back to the GUI, and supports undo/redo. Inversion intentionally
maps an inactive zero scalar to a nonzero output in this authored route.

Focused evidence: target/authoring-advanced-patch-focused.log. This exercises
native edit commands, not manual frontend form interaction or physical output.
Color, indexed and fixture routes through advanced replacement remain separate
acceptance work.

### Advanced color, indexed and fixture replacement acceptance

Extended the existing realistic native authoring scenarios through ReplacePatch.
RGBW and discrete component orders are reversed and their exact output bytes
checked; repeated component indices reject without changing the accepted session.
Both mirrored indexed routes receive new normalized mappings, with duplicate IDs
rejected and corrected output verified. RGB/RGBW fixture graphs receive explicit
cell ranges and new node/edge IDs; incorrect encoder widths reject, while corrected
replacements retain profile identity, behavior rules and typed overrides.
All three scenarios check undo/redo, typed save/reload equality and rendered bytes.

Focused evidence: target/authoring-advanced-color-indexed-focused.log and
target/authoring-advanced-fixture-focused.log. These are native command checks;
manual patch and fixture form walkthroughs remain separate acceptance work.

Packaged fixture form acceptance: created walkthrough_fixture_1 in the local
project-copy walkthrough. Added two dimmer behavior rules and submitted; the form
reported 'Function 1 has more than one behavior rule' while retaining the draft.
Removed one rule and saved successfully. Reload From Disk retained the profile
with one function, one channel and one behavior rule; Edit profile reopened it.
UI inventory: target/authoring-fixture-form-reopened.txt. The packaged app was
closed after the walkthrough. This covers basic form recovery, not every fixture
control or physical output.

### Packaged advanced patch draft recovery

In the project-copy walkthrough, opened Advanced patch editor and changed the
first 8-bit filter input width from 90 to 89. Apply reported that node 3 output
did not match node 4 input and retained the editable draft. Corrected the width
to 90 and changed component order from [0,1,2] to [1,0,2]. Apply succeeded.
Reload From Disk and reopening the editor retained width 90 and the new order;
UI value evidence is target/authoring-patch-form-reopened.txt. Closed the packaged
app afterward. No live output was enabled in this walkthrough.

This verifies existing-node edits and correction through the actual form. New
node/connection creation and other filter-specific forms remain separate manual
acceptance work. No production code changed, so the previous full gate remains
the applicable software validation.

### Packaged new patch route authoring

Built a route entirely in Advanced patch editor: added a source, selected the
existing New dimmer scalar element, set scalar width 1, added scalar-to-components
and 8-bit filters, and added controller 2 output at channel 9 with width 1.
Created connections 24 -> 25 -> 26 -> 27 using the node selectors. Apply succeeded
and saved. The saved patch has the four new nodes and three edges; evidence is
target/authoring-new-route-saved.txt. Reload From Disk showed assigned channels
1-1, 2-2, 3-8, 9-9 and free channels 10-512. No live output was enabled.
The packaged app was closed afterward. This adds manual source/filter/sink and
connection creation to the prior existing-node draft recovery evidence.

### Packaged RGBW fixture and shutter authoring

Created walkthrough_color_fixture_1 through the profile form: Color function 1
with RGBW mixing and four component channels; indexed Shutter function 2 with
Closed/Open entries at DMX 0/255 and a coarse channel; a shutter rule references
those entries. An overlapping entry range was rejected while retaining the draft;
correcting the range allowed saving. Created Walkthrough fixture as setup element
6 using that profile and assigned it to controller 2, port 1, channels 10-14.
Reload From Disk retained the profile reference, fixture and assignment, reporting
free channels 15-512. Evidence: target/authoring-color-fixture-saved.txt and
target/authoring-color-fixture-reopened.txt. Closed the packaged app afterward.
No live output or physical device was used. Remaining manual fixture controls
include fine channels, custom curves, color-wheel mappings and prism behavior.

### Precision fixture form and user instructions

Created walkthrough_precision_fixture_1 through the packaged profile form: one
range dimmer function, custom curve (0,0), (0.5,0.25), (1,1), coarse channel 1 and
fine channel 2. Saved and reloaded, then reopened the profile for editing. Evidence:
target/authoring-precision-fixture-saved.txt and
target/authoring-precision-fixture-reopened.txt. Closed the app afterward.

Expanded docs/first_show.md with fixture function/channel/behavior authoring,
validation recovery, profile selection, guided assignment, and custom patch route
instructions. The examples correspond to the verified packaged workflows. No
production source changed in this increment. Wheel and prism form controls remain
manual acceptance work; physical device acceptance is still separate.

### Packaged wheel and prism profile form

Created walkthrough_wheel_prism_1 with a wheel function, White entry and a white
color-to-entry mapping, plus an indexed prism function with Disabled/Enabled
entries at DMX 0/255. Selected the prism function and its two entries in the
behavior form. Saved and reloaded, then reopened the profile. UI values confirmed
Entry: White (1), Disabled entry: Disabled (1), Enabled entry: Enabled (2).
Evidence: target/authoring-wheel-prism-saved.txt and
target/authoring-wheel-prism-reopened.txt. Closed the app afterward. This closes
the named wheel/prism form walkthrough gap; native frame encoding remains covered
by fixture_color_acceptance.rs, and no physical output is claimed.

### Remaining patch filter forms and acceptance review

Inserted node 31 in the custom dimmer route through the packaged editor. Applied
scale/invert with scale 0.5 and invert enabled; then applied fan-out with three
outputs. Finally selected gamma dimming with exponent 2. Converted node 26 to
16-bit output in fine/coarse order and moved its two-channel sink to channels
15-16. Changed the indexed mapping for option 1 to 0.5. Selected the fixture
profile in its encoding filter, verified invalid width 6 was rejected at the
encoder-to-sink connection, and corrected it to 5.

Reload and reopening the form retained indexed value 0.5, Fine/coarse byte order,
profile width 5 and gamma exponent 2. Evidence: target/authoring-filter-forms-saved.txt,
target/authoring-filter-forms-final.txt and target/authoring-filter-forms-reopened.txt.
Closed the packaged app. No live output was enabled.

The named software walkthrough gaps are now covered by the native acceptance and
packaged form evidence. The remaining goal acceptance is physical device work.
COM4 remains a CP210x bridge; board/wiring/network details and confirmation that
its firmware may be replaced are still needed. Existing desktop and separate
firmware check logs were rechecked; no production code changed during the manual
walkthrough increments. See docs/authoring_acceptance_status.md for requirement
scope and the physical acceptance sequence.

### Goal completion and physical scope clarification

On 2026-09-08, the user clarified that physical acceptance is not required for this
goal. Reviewed the software requirement matrix, passing desktop gate (55 desktop
tests), separate output-enabled firmware check and packaged walkthrough evidence.
The software goal is complete. Physical installation, power-cycle recovery and
LED validation remain unclaimed and outside this goal. Earlier history entries
that treated those steps as a completion blocker are superseded by this scope
clarification. No further hardware details or firmware approval are needed here.
