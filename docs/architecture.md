# Architecture

Donder turns a folder of source documents into frames of LED color. Each stage
has one owner:

```text
.donder documents ──► donder-project-io ──► DonderProject (typed, authoritative)
                         load, check              │  GUI edits, undo, save
                                                  ▼
                                         donder-elaboration ──► PreparedSequence
                                         prepare(selection)          │
                                                                     ▼
                             donder-runtime: playback.evaluate(time) ──► colors and output bytes
                             (preview process, live output, ESP32)
```

## Crates

| Crate | Owns |
| --- | --- |
| `donder-language` | Domain types, the [effect language](effect_language.md) compiler and optimizer, bytecode, parameter binding, sequence validation, shared sampling math. Its portable values and programs are `no_std + alloc`; the compiler and authoring model sit behind the default `host` feature. It does not depend on the runtime. |
| `donder-project-io` | Source documents, imports, linking, diagnostics, YAML serialization, saving and project copies. See [project format](project_format.md). |
| `donder-elaboration` | Resolving a sequence and an output selection into a `PreparedSequence`: targets, fixture geometry, bound invocations, automation, specialization, fusion, retention. See [output selection](output_selection.md). |
| `donder-runtime` | `no_std` prepared-sequence playback, the private batched VM, and the prepared archive format (`archive.rs`). |
| `donder-preview` | The native Preview process: archive decoding, playback and the wgpu renderer. |
| `donder-output` | E1.31 and Art-Net transports. |
| `donder-cli` | `donder init`, `check` and `copy`. |
| `apps/desktop` | The Tauri app: workflows, background scheduling and history in `desktop_state` and `state_tasks`; typed GUI projection and edits in `gui`; the React frontend. |
| `firmware/esp32` | The controller firmware, a separate Cargo workspace. See [ESP32 loading](esp32_loading.md). |

Each crate's `lib.rs` is its public facade; implementation modules are not
alternate entry points.

## Editing

After loading, the typed `DonderProject` is the model. `SourceProject` records
which document owns each object, the import graph, original DSL text and asset
references; it is not a second editable model.

Loaded and historical states are immutable `Arc<ProjectSession>` snapshots. A
GUI edit:
1. deep-clones the current session once;
2. mutates that candidate's typed state;
3. validates the edit;
4. publishes the accepted snapshot to state, history, save and render work.

A rejected edit leaves the previous snapshot untouched. GUI edits never touch
YAML text, never run project checks, and never reload from disk. Saving and
render refresh are scheduled from the accepted revision through the single
latest-request scheduler in `state_tasks/`.

Project workflows own ordering, external-edit preconditions and rollback. A
rename publishes its new paths even if a later preference refresh fails, and a
failed staging restore reports where the original payload was kept. The desktop
accepts only the current preferences format; there are no migrations.

## Preparation and playback

`donder_elaboration::prepare(&project, &sequence_id, outputs)` returns
`Option<PreparedSequence>`, with `None` meaning the selection cannot be
resolved. `outputs` is `PrepareOutputs::All`, `Controllers` or `Ports`.
Preparation resolves everything symbolic (imports, names, layouts, groups,
device selection) so that playback never does.

`sequence.into_playback()` creates runtime-owned scratch and output storage once.
`playback.evaluate(time)` then returns borrowed fixture colors and packed
controller bytes for that frame. It cannot fail and does not allocate. The VM,
execution plan and storage layout are private to the runtime; callers build
sequences only through `SequenceBuilder`'s owner-bound handles.

The same `PreparedSequence` runs in three places:

- **Preview.** The desktop sends the encoded archive, projected fixture
  geometry and clock anchors to a separate winit/wgpu process when their
  revisions change. That process evaluates frames itself, so pixels never cross
  Tauri IPC. Audio state arrives as timestamped anchors that the Preview
  interpolates and corrects to. A separate process also keeps GTK and wgpu from
  sharing one Wayland surface.
- **Live output.** The desktop service evaluates controller bytes and sends them
  over E1.31 or Art-Net. Output is opt-in for each run and fails closed: stopping
  blacks out active ports and terminates E1.31 streams.
- **Controllers.** Exported archives, prepared for a controller's ports, are
  uploaded to the ESP32 loader.

Archive decoding checks the envelope, checksum, archive structure and resource
budgets. It trusts a compatible Donder producer for graph and bytecode
correctness, because the only producer is the runtime's own builder. The format
is a numbered current version; a mismatch is rejected rather than migrated.

Performance design is described in [performance](performance.md).
