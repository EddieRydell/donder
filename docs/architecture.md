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
| `donder-runtime-types` | What playback accepts and what it means, `no_std + alloc`: colors, curves, gradients, marks and sample time; the strip bytecode (`bytecode`); program, invocation and parameter-binding descriptions; prepared inputs such as sequence timing, fixture and target geometry and output encodings; and the sampling math (`sampling`) shared by constant folding and evaluation. An item belongs here only if the runtime consumes it and an upstream crate produces it. |
| `donder-language` | The text language. `data`: the data-document syntax tree, parser, printer, literal rules and schema traits. `compiler`: the [effect language](effect_language.md) and its [compiler](effect_compiler.md) (dataflow IR, specialization, scheduling, bytecode emission). `analysis`: semantic tokens, symbols, hover and completion for editors. The root holds import syntax, naming rules and authored quantities (times, distances, transforms). It does not depend on the model or the runtime. |
| `donder-model` | The typed domain model: fixtures, layouts, patches, controllers, setups, sequences, effect and operator definitions, ownership, identity, geometry preparation, validation, and the validated `ProjectEdit` batches and ownership edits that change it. |
| `donder-project-io` | Source documents, imports, linking, diagnostics, the data-document schema (`document/types.rs`, derived by `donder-data-derive`), loading declarations into typed state, printing them back, saving and project copies. See [project language](project_language.md). |
| `donder-elaboration` | Resolving a sequence and an output selection into a `PreparedSequence`: targets, fixture geometry, bound invocations, automation, the global signal graph (black-signal folding, operator fusion), retention. See [output selection](output_selection.md). |
| `donder-runtime` | `no_std` prepared-sequence playback, the private strip interpreter, and the prepared archive format (`archive.rs`). |
| `donder-output` | E1.31 and Art-Net transports, and FSEQ v2 export for FPP. |
| `donder-audio-analysis` | Beat and downbeat detection from song audio: decoding, the log-mel spectrogram and the embedded Beat This! model (small, MIT licensed) run with tract. Host-only. |
| `donder-language-server` | The language server for data documents and scripts, independent of its transport. See [Text editing](#text-editing). |
| `donder-cli` | `check`, `copy`, `export-fseq`, `lsp` (the language server over stdio) and the generated builtin reference. |
| `donder-editor` | Typed GUI projection, edits, selection, clipboard and model conversion, shared by the desktop and browser hosts. Edit helpers mutate the candidate session they are given. |
| `donder-sequence-api` | The serialized editor contract (DTOs and `SequenceGuiEdit`), exported to TypeScript for both hosts. |
| `donder-browser` | A WASM session for the website: an in-memory project, editing through `donder-editor`, preparation and playback. |
| `donder-test-support` | Dev-only playback workloads shared by runtime and elaboration tests and benchmarks. |
| `apps/desktop` | The Tauri app: workflows, background scheduling and history in `desktop_state` and `state_tasks`, platform IO, the Preview process (`preview/`: playback clock, wgpu renderer and pipe protocol), and the React frontend, which exports the editor UI as `@donder/editor` for both hosts. |
| `firmware/esp32` | The controller firmware, a separate Cargo workspace. See [ESP32 loading](esp32_loading.md). |

Each crate's `lib.rs` is its public facade; implementation modules are not
alternate entry points.

## Editing

After loading, the typed `DonderProject` is the model. `SourceProject` records
which document owns each object, the import graph, script source text and asset
references; it is not a second editable model. Loading assigns every object a
fresh session identity; names in the text resolve to them, and saving prints
names back. Data documents are therefore exactly the typed state: saving prints
the canonical text of each project-owned data document, and scripts are kept
byte for byte.

Loaded and historical states are immutable `Arc<ProjectSession>` snapshots. A
GUI edit:
1. deep-clones the current session once;
2. mutates that candidate's typed state;
3. validates the edit;
4. publishes the accepted snapshot to state, history, save and render work.

A rejected edit leaves the previous snapshot untouched. GUI edits never touch
document text, never run project checks, and never reload from disk. Saving and
render refresh are scheduled from the accepted revision through the single
latest-request scheduler in `state_tasks/`.

Project workflows own ordering, external-edit preconditions and rollback. A
rename publishes its new paths even if a later preference refresh fails, and a
failed staging restore reports where the original payload was kept. The desktop
accepts only the current preferences format; there are no migrations.

## Text editing

Both hosts edit text in Monaco, and every language feature comes from
`donder-language-server`: diagnostics with quick fixes, formatting, semantic
tokens, hover, definition, references, rename, completion, the outline and
signature help. Semantic tokens are the only highlighting; there is no
TextMate or Monarch grammar.

The server is a plain `Server::handle(message) -> replies` loop with no async
runtime. The host calls `Server::idle` once messages pause for
`IDLE_DELAY_MS`; that is when the project is checked again and diagnostics are
published. Its features read:
- the project check's `ProjectIndex`, which links each name in a data document
  to what it resolved to, for navigation across documents;
- `donder_language::analysis`, which works from partial parses and tokens, so
  scripts and data documents keep completion and signature help while broken.

Each host supplies a transport and a `DocumentSource`, which gives the server
the host's unsaved text ahead of the disk:

| Host | Server | Transport |
| --- | --- | --- |
| Desktop | A thread in the app process, reading `DesktopState`'s working copies | The `language_server_send` command and the `language_server_message` event |
| Website | The WASM `LanguageServer` in a worker, with no project | Worker messages |
| Other editors | `donder lsp` | stdio |

The frontend's `LanguageClient` (`ui/source/languageClient.ts`) maps the
protocol onto Monaco's providers. Edits to documents Monaco has not opened,
such as a rename's, go to `apply_text_edits`, which opens them as unsaved
working copies.

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

The same `PreparedSequence` runs in three places. All three map wall time to
show time and choose rendered frames through the runtime's `PlaybackRate`, so a
playback speed and its frame timing (scaled or constant) behave identically
everywhere:

- **Preview.** The desktop sends the encoded archive, projected fixture
  geometry and clock anchors to a separate winit/wgpu process when their
  revisions change. Each pipe message is a JSON header and a binary payload;
  the archive and fixture instances travel as raw bytes. That process evaluates frames itself, so pixels never cross
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
