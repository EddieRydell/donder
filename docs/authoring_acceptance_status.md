# Setup and authoring acceptance status

The setup and authoring goal is **complete within its agreed software scope**.
On 2026-09-08, the user explicitly excluded physical acceptance from this goal.
The evidence below covers implementation, automated checks and packaged app
walkthroughs; it does not claim physical hardware validation.
Implementation history is in [first_class_authoring.md](first_class_authoring.md);
the user walkthrough is [first_show.md](first_show.md).

## Evidence reviewed

| Goal requirement | Current evidence | What the evidence establishes / remaining limit |
| --- | --- | --- |
| Setup identity and dependency ownership | `gui/document.rs`, `gui/setup.rs`; `desktop_state/authoring_acceptance.rs` | Shared workspace/module resolution, owned mutations, explicit controller/layout copies, and dependency-file preservation. Imported documents remain read-only. |
| Blank project, two props, controller, effect, playback, save/reopen | `desktop_state/authoring_acceptance.rs`; local walkthrough captures | Automated typed authoring, expected controller output, and persistence. Earlier packaged walkthrough and localhost capture show only Prop A illuminated. This is not physical LED evidence. |
| Prop shape, placement, duplication, grouping and routing | `dawn-language/src/setup/authoring.rs`, `authoring/routing.rs`; authoring and color-prop acceptance | Guided routes resize with props; conflicts reject the candidate; copies and geometry use the existing typed/history path. |
| RGBW and discrete capabilities | `desktop_state/color_prop_acceptance.rs` | Creation, conversion, width changes, output order, discrete mapping changes, undo/redo, exact bytes and reload. Discrete effects still require exact color mappings. |
| Controllers and output assignment | Setup authoring and control-output acceptance | Guided destination selection, range checks, overlap rejection, replacement and mirrored routes. Explicit raw channel tests operate independently of a sequence. |
| Audio-independent transport and preview | `desktop_state/audio.rs`; `sequenceAudioSync.test.js`; earlier preview walkthrough | Silent duration, play/pause, prepared target/frame, ordered selection changes and copy-to-playback. Async frontend tests control command timing; they are not a manual native decode-race reproduction. |
| Live-output verification | `desktop_state/output_test.rs`, `output/live.rs`; local UDP captures | Exact selected-port bytes, blackout, expiry, stop acknowledgement, failure retention and resume ordering. Art-Net is exercised in loopback tests and E1.31 in the packaged localhost capture. |
| Scalar/indexed controls | `desktop_state/control_output_acceptance.rs` | GUI edits, clips, channel values, inactive mappings, resizing, mirrored destinations, undo/redo and persistence. |
| Fixture profiles and typed controls | `desktop_state/fixture_copy_acceptance.rs`, `desktop_state/fixture_color_acceptance.rs`; fixture GUI editors | Native GUI-command acceptance covers range/indexed functions, coarse/fine channels, grouped clips, shared-profile copying, RGB/RGBW mixing, wheel mappings, all four behavior rules, explicit overrides and exact encoded bytes through reload. Packaged forms cover a dimmer profile, duplicate-behavior recovery, an RGBW/indexed-shutter profile with DMX-range correction, fixture creation, guided assignment, a coarse/fine profile with a three-point custom curve, wheel mapping and prism entry selection. Reload preserves the authored profiles and selected entry identities. Physical fixture output remains outside this evidence. |
| Advanced patch editing | `desktop_state/advanced_patch_acceptance.rs`, color-prop, control-output and fixture-color acceptance; `ui/gui/setup/PatchEditor.tsx` | Native replacement acceptance covers scalar cell ranges/conversion, gamma/custom curves, scale/invert, fan-out, 8-bit and both 16-bit byte orders, RGBW/discrete breakdown and component reordering, indexed mappings and RGB/RGBW fixture encoding. Cases verify exact bytes, invalid drafts, correction, undo/redo and reload. Fixture replacement also checks explicit cell selection and changed node/edge identities. Packaged forms cover node/connection creation, component order, scale/invert, fan-out, gamma, 16-bit byte order, indexed mapping, fixture profile selection/width validation and reload. These are representative valid routes, not every possible graph. |
| Editable dependency copy | `desktop_state/project_copy_acceptance.rs`, `dawn-project-io/tests/editable_copy.rs`, `desktop_state/audio.rs` | Save/Discard/Cancel handling, source and asset copying, nested imports, typed/frame preservation, and playback/rendering after opening the editable copy. |
| Embedded sequence selection/export | `desktop_state/sequence_export.rs`, `dawn-elaboration/tests/output_selection.rs` | Explicit selected ports, duplicate/stale/unavailable selection rejection, compaction and retained generator dependencies. |
| Device installation/provision/upload/transport | `device/firmware.rs`, `device/provisioning.rs`, `device.rs`; `docs/esp32_loading.md` | Bundled image checks, hardware admission checks, serial exchanges and authenticated HTTP behavior have host-side coverage. Physical desktop installation and device interaction remain unverified. |
| Persistent device storage and recovery | `dawn-device-storage/src/tests.rs`; firmware loader | Simulated remounts, torn writes, corrupt records, credential replacement and explicit erase. This does not prove physical flash persistence, power-cycle recovery or timing under live I2S output. |
| Actionable diagnostics | `dawn-project-io/tests/diagnostics.rs`; output failure and GUI ownership paths | Source locations, unknown setup fields, transaction rejection, controller errors and an explicit editable-copy instruction. Diagnostics coverage is evidence for these cases, not proof that every error message is adequate. |

Paths above are relative to `apps/desktop/src`, `apps/desktop/frontend/src`, or
`crates` as indicated by their module names. The primary desktop gate is
`cargo fmt` followed by `pnpm check`. The current reviewed passing log is
`target/authoring-advanced-routes-check.log` (55 desktop tests).

## Optional physical acceptance outside this goal

Physical installation, power-cycle recovery and LED output have not been verified
in this acceptance run. They are outside the agreed goal and do not block its
completion. A future hardware run would need the board identity, wiring, network
details and authorization to replace its firmware.

For a future hardware run, use the existing bundled-image installer and device UI:

1. Record board identity, selected port, output wiring and the bundled image hash.
2. Install and verify the image through Dawn; record the installer result.
3. Provision the intended network and verify authenticated device capabilities.
4. Export/upload the selected outputs, then verify play, pause, resume and stop
   against actual lights and reported transport state.
5. Power-cycle and verify saved credentials and sequence recovery, including
   playback without the desktop attached.
6. Verify explicit saved-data recovery and re-provisioning on the dedicated board.
7. Record representative frame deadlines, memory use and output behavior with
   persistent storage enabled. Keep electrical/waveform claims separate from
   software frame counters.

The ESP32 workspace is separate from the desktop gate. Its current source must
also pass the documented `cargo +esp check --release --bin loader --features
i2s-output --locked` command from `firmware/esp32`; a host gate cannot substitute
for that check or for the physical steps above.

Current audit result: the output-enabled firmware check passed; evidence is
`target/authoring-completion-firmware-check.log`. The bundled image SHA256 was
recomputed and matches its committed checksum:
`36C67E8249EDD215C4421520D20578B54D5DFB0855A021F6D1E27B95872A62CE`.
Neither check contacted or reset the board.

## Software completion

Fixture color mixing, wheel mappings and behavior rules now have the same native
GUI-edit-to-render-to-save acceptance path as the range/indexed fixture scenario.
Advanced scalar, color, indexed and fixture replacement now also have native
authoring/render/save acceptance. Fixture form walkthroughs include the four
function kinds, coarse/fine and color-component channels, custom curves, and all
four behavior kinds. Patch walkthroughs cover new routes, correction of rejected
drafts, and the filter-specific settings listed above. The user guide now explains
fixture creation and custom routing alongside the first pixel show.

All software requirements in the goal have implementation and acceptance evidence
listed above. The desktop gate, separate output-enabled firmware check, native
authoring/render/persistence acceptance and packaged form walkthroughs passed.
Physical testing is excluded by the user's scope clarification; no hardware
information or firmware-replacement approval is needed to close this goal.
