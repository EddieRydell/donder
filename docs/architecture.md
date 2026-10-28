# Architecture

Donder turns a folder of source documents into frames of LED color (the five
top-level calls are listed in the [code map](code_map.md)). Each stage
has one owner:

```text
.donder documents ──► donder-project-io ──► DonderProject (typed, authoritative)
                         load, check              │  GUI edits, undo, save
                                                  ▼
                                         donder-elaboration ──► PreparedShow
                                         prepare(selection)          │
                                                                     ▼
                             donder-runtime: playback.evaluate(time) ──► colors and output bytes
                             (preview process, live output, ESP32)
```

## Crates

| Crate | Owns |
| --- | --- |
| `donder-runtime-types` | What playback accepts and what it means, `no_std + alloc`: colors, curves, gradients, marks and sample time; the strip bytecode (`bytecode`); `Program` (bytecode and its signal input count; an effect's has none) and `BoundProgram` (a program bound to checked parameter values and automation); prepared inputs such as sequence timing, fixture and target geometry and output encodings; and the sampling math (`sampling`) shared by constant folding and evaluation. An item belongs here only if the runtime consumes it and an upstream crate produces it. |
| `donder-language` | The text language. `data`: the data-document syntax tree, parser, printer, literal rules and schema traits. `compiler`: the [effect language](effect_language.md) and its [compiler](effect_compiler.md) (dataflow IR, specialization, scheduling, bytecode emission). `analysis`: semantic tokens, symbols, hover and completion for editors. The root holds import syntax, naming rules and authored quantities (times, distances, transforms). It does not depend on the model or the runtime. |
| `donder-model` | The typed domain model: fixtures, layouts, patches, controllers, input devices, setups, sequences, rigs, effect and operator definitions, ownership, opaque object ids, geometry preparation, validation, and the validated edits that change it: `ProjectEdit::Put` and `ProjectEdit::Remove` batches over one `Object` enum, and the ownership edits. |
| `donder-project-io` | Source documents and object addresses, imports, linking, diagnostics, the data-document schema (`document/types.rs`, derived by `donder-data-derive`), loading declarations into typed state, printing them back, saving and project copies. See [project language](project_language.md). |
| `donder-elaboration` | Resolving a sequence or a rig and an output selection into a `PreparedShow`: targets, fixture geometry, bound programs, automation, the global signal graph (black-signal folding, operator fusion), retention. Sequences and rigs share one preparation (`stage::build`) and differ only in how they place their clips. See [output selection](output_selection.md). |
| `donder-runtime` | `no_std` show playback without `alloc`: a trusted show image borrowed in place (`Image`), and evaluation into one caller-provided buffer (`Playback`) by the private strip interpreter. The image's record layouts (`format`) are private to it. The default `host` feature adds the image builder (`ShowBuilder`) and the owned `PreparedShow` and `ShowPlayback`. |
| `donder-output` | E1.31 and Art-Net transports, and FSEQ v2 export for FPP. |
| `donder-device-protocol` | The editor-controller wire protocol, `no_std`: HTTP paths and the token header, the JSON commands and replies, and the UDP clock packets. The desktop and the firmware both use it. |
| `donder-audio-analysis` | Beat and downbeat detection from song audio: decoding, the log-mel spectrogram and the embedded Beat This! model (small, MIT licensed) run with tract. Host-only. |
| `donder-language-server` | The language server for data documents and scripts, independent of its transport. See [Text editing](#text-editing). |
| `donder-cli` | `check`, `copy`, `render` (fixture colors at chosen times), `export-fseq`, `lsp` (the language server over stdio) and the generated builtin reference. |
| `donder-editor` | Typed GUI projection, edits, selection, clipboard and model conversion, shared by the desktop and browser hosts, and in its `dto` module the serialized editor contract exported to TypeScript. Edit helpers mutate the candidate session they are given. |
| `donder-test-support` | Dev-only playback workloads shared by runtime and elaboration tests and benchmarks. |
| `apps/desktop` | The Tauri app: workflows, background scheduling and history in `desktop_state` and `state_tasks`, platform IO, the Preview process (`preview/`: playback clock, wgpu renderer and pipe protocol), and the React frontend, which exports the editor UI as `@donder/editor` for both hosts. |
| `firmware/esp32` | The controller firmware, a separate Cargo workspace. See [ESP32 loading](esp32_loading.md). |

Each crate's `lib.rs` is its public facade; implementation modules are not
alternate entry points.

## Editing

After loading, the typed `DonderProject` is the model. `SourceProject` records
the import graph, script source text, asset references and each document's
declared objects by id and name; it is not a second editable model. Loading
assigns every object a fresh session identity, an opaque id from one
project-wide counter, so references and the objects they name agree whatever
order they are read in. The model knows objects only by id: a declared object's
document and name live in the source inventory alone, and an owned object's
address is its owner's address and slot. Moving a document or making an object
reusable changes where it is written, not the references to it; making one
independent copies it under fresh ids. Saving prints names back from the ids,
so data documents are exactly the typed state: saving prints the canonical text
of each project-owned data document, and scripts are kept byte for byte. State
that outlives a reload, such as the desktop's render target, keeps addresses
and resolves them in each new session.

Loaded and historical states are immutable `Arc<ProjectSession>` snapshots. A
GUI edit:
1. deep-clones the current session once;
2. mutates that candidate's typed state;
3. validates the edit;
4. publishes the accepted snapshot to state, history, save and render work.

The model has one store per kind of reusable object, keyed by id; owned
objects live in their owner. `DonderProject::get::<T>(id)` finds an object of
kind `T` wherever it is written and `objects::<T>()` lists every one, so there
is one lookup for every kind. Every model edit is a batch of `ProjectEdit`s:
`Put(Object)` writes an object under the id it carries, in place of the object
with that id or as a new reusable object, and `Remove(ObjectId)` removes a
reusable one; the batch is validated as one change. Every domain rule lives once
in the model's validation. A broken rule is a `ProjectValidationError`: the
object it concerns and a typed `ValidationProblem` that names the member
(a clip, layer, voice, control, port, layout item, ...) and the parameter or
input where the problem lies; its sentence is written once, in `Display`. The
model refuses an edit it cannot apply with an `EditRejection`. Ownership edits (make
reusable, make independent, use an existing declaration) name a slot by its owner and
the member it holds and keep references consistent through
`DonderProject::for_each_reference`, the one listing of every reference from one
object to another, which project IO also uses to keep imports visible.

The GUI addresses an object by its document path, declared name and owned path,
and the object's kind picks its editor. `apply_edit` is the one entry for GUI
edits: kind-specific edits (`GuiEditCommand::Sequence`, `Layout`, ...), the
ownership edit, and the generic item verbs `Rename`, `SetDescription`,
`SetColor` and `Delete`, which name a part of the open object with a `GuiItem`.
Clip edits take a list of clip references, so a single clip is a list of one.
Every edit returns one `GuiEditUpdate`: the changed document, the selection a
selection edit leaves, what a cut or paste took, and the names it created.

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
runtime. Requests answer at once from the current texts and the last project
check; they never wait for a new check. Each document keeps what the server
derives from its text (line index, syntax tree, tokens, script analysis) until
the text changes, so requests on a very large document do not lex or parse it
again. Clients sync documents incrementally, and data documents are colored
through `semanticTokens/range`, only where they are shown. The host calls
`Server::idle` once messages pause for `IDLE_DELAY_MS`; that is when the
latest check is taken and its diagnostics published. Features read:
- the check's `ProjectIndex`, which links each name in a data document to what
  it resolved to, for navigation across documents. It is used only while every
  open document still has the text the check read; definition, references and
  rename first take a new check;
- `donder_language::analysis`, which works from partial parses and tokens, so
  scripts and data documents keep completion and signature help while broken.

Who checks the project depends on the host:

| Host | Server | Checks | Transport |
| --- | --- | --- | --- |
| Desktop | `Server::with_source`, a thread in the app process | `DesktopState`, whose working-copy check the server publishes through its `DocumentSource` | The `language_server_send` command and the `language_server_message` event |
| Website | The WASM `LanguageServer` in a worker, with no project | The server | Worker messages |
| Other editors | `donder lsp` | The server | stdio |

Every check goes through `check_project(root, overrides, cache)`, the one
entry point, with a `ProjectCheckCache` that its owner keeps, so a check after
an edit parses only the documents whose text changed. A check reads each
document once, its override or its file, and loading, the per-document syntax
report and the unreferenced-document warnings all read that text.
`ProjectCheckReport::into_session` gives the loaded project or its
diagnostics; loading reports nothing but `IoDiagnostic`s, which are also the
diagnostics the desktop serializes to its frontend and the language server
publishes. Loading resolves names to ids, converts documents into typed state
and records links; the model then validates the project, and an error is placed
at the text of the object, member and field it names. The desktop's text editor sends Monaco's edits to
`update_document` as they happen, and snapshots carry no document text: the
editor reads it with `document_text` when it opens a document and when the
document's revision moves for another reason, such as a GUI edit or a reload.

The frontend's `LanguageClient` (`ui/source/languageClient.ts`) maps the
protocol onto Monaco's providers. Edits to documents Monaco has not opened,
such as a rename's, go to `apply_text_edits`, which opens them as unsaved
working copies.

## Preparation and playback

`donder_elaboration::prepare(&project, &sequence_id, retain)` returns
`Option<PreparedShow>`, with `None` meaning the selection cannot be
resolved; `donder_elaboration::arm` prepares a rig the same way. `retain` (which hardware outputs the image keeps) is `Retain::All`, `Controllers` or `Ports`.
Preparation resolves everything symbolic (imports, names, layouts, groups,
device selection) so that playback never does.

A prepared show, a sequence's or a rig's, is a show image: a header, a section table and sections of
fixed-size records that refer to one another by index. Playback reads the
records where they lie, in memory or in a controller's flash. A show's
`Layout` (`Image::layout`) gives the exact size of its playback buffer;
`Playback::new` lays the nodes' frame buffers, their state buffers, scan
frames, port bytes, the strip workspace and the automation scratch out in that
buffer, and `Playback::resume` borrows them again on a later frame.
`prepared.into_playback()` (a `ShowPlayback`) owns both on the host. `evaluate(time)` returns
borrowed fixture colors and packed controller bytes for the frame nearest
`time`; asking for the same frame again returns the frame it holds. It cannot
fail and does not allocate. The VM and record layouts are private to the
runtime; callers build images only through `ShowBuilder`'s owner-bound
handles: one `clip` call places a bound program on a target for a while,
`operator` and `mix` build the graph, and `route` packs a target into a
port through an optional gamma table, which the builder writes once. Equal
programs are written once.

Evaluation pushes whole frames through the signal graph. The nodes run in
topological order, each into a frame buffer that is reused once its last
reader has run (at most 16 at once): a layer max-composites its active clips,
found by a binary search over its clips' start order and latest ends; an
operator runs over every pixel and reads its inputs' frames of this frame at
any pixel; a mix keeps each channel's maximum. A layer or operator that reads
`previous` (at most 16) copies its frame into a state buffer once it has run,
which it reads on the next frame. Colors are 16 bits per channel throughout;
only packing the output bytes narrows them, through each route's gamma lookup.
The builder asserts the limits on frame buffers, state buffers and scans per
program (8).

The same `PreparedShow` runs in three places. All three map wall time to
show time and choose rendered frames through the runtime's `PlaybackRate`, so a
playback speed and its frame timing (scaled or constant) behave identically
everywhere:

- **Preview.** The desktop sends the encoded image, projected fixture
  geometry and clock anchors to a separate winit/wgpu process when their
  revisions change. Each pipe message is a JSON header and a binary payload;
  the image and fixture instances travel as raw bytes. That process evaluates frames itself, so pixels never cross
  Tauri IPC. Audio state arrives as timestamped anchors that the Preview
  interpolates and corrects to. A separate process also keeps GTK and wgpu from
  sharing one Wayland surface.
- **Live output.** The desktop service evaluates controller bytes and sends them
  over E1.31 or Art-Net. Output is opt-in for each run and fails closed: stopping
  blacks out active ports and terminates E1.31 streams.
- **Controllers.** Sequence images, built for a controller's ports, are
  uploaded to the ESP32 loader, which stores them in a flash slot and plays
  them from a copy in RAM beside their playback buffer.

Perform mode plays a rig instead of a sequence (`desktop_state/perform.rs`).
Starting arms the rig on the active setup and starts a session clock; sequence
playback stops until the performance ends. Keys from the main window and from
the Preview (forwarded over its event pipe) are stamped on the session clock
when the desktop receives them, routed by the performance's `RigRouter`
(`desktop_state/perform/router.rs`, which reads the model's trigger lengths,
voice policies and played quantities directly), and the
resulting `RigEvent`s fan out in order to every evaluator: the live output
worker plays its own `ShowPlayback` on the session clock, and the Preview
receives the rig image as its content, then the events as they arrive, with
the session time as its clock anchor. An evaluator that joins mid-performance
replays the performance's events from the start. Frame caches include the
events applied, since an event changes a frame without changing its time.

The image is trusted: only the host builder writes it, so `Image::open` checks
nothing but its format version and playback indexes records without checking
them. The header holds an `id`, a hash of the image the builder writes, which a
controller reports so a host can tell which show it holds; playback never reads
it. A rig is an image with voices, live mark
collections and controls; their commands change only the playback buffer. A
control sets automation bindings, and a live collection is read by
automation bindings of marks parameters (`AutomationMapping::LiveMarks`). The format is a numbered current version; a mismatch is rejected
(`LoadError::Version`) rather than migrated.

Performance design is described in [performance](performance.md).
