# Code map

The five top-level calls of the system, each with the function that makes it
and the named steps inside it. The [architecture](architecture.md) describes
what each step owns.

## Load a project

`donder_project_io::check_project(root, overrides, cache)` in
`crates/donder-project-io/src/project_loading.rs` returns a
`ProjectCheckReport`; `into_session()` gives the `ProjectSession` (the typed
`DonderProject` and its `SourceProject`) or the diagnostics.

1. `ProjectMetadata::parse` reads `project.data.donder`, and
   `ProjectWorkspace::new` validates the project configuration.
2. `Loader::load` (`loader/mod.rs`): `load_document` follows the imports from
   the root document, compiling each script with `compile_script`;
   `build_document_scopes` links names across documents; `resolve_project` and
   `resolve_loaded_objects` turn declarations into typed objects
   (`loader/resolve.rs`: `owned_setup`, `owned_sequence`, `owned_rig`, ...).
3. `DonderProject::try_new` validates the project (`validate_project`) and
   admits its playback inputs (`ProjectInputs::admit`: each clip's and
   operator's `Invocation`).
4. `analyze_project_documents` reports per-document syntax problems,
   `unreferenced_documents` warns about documents nothing imports, and
   `sort_diagnostics` orders the `IoDiagnostic`s.

## Compile a script

`donder_language::compiler::compile_script(source)` in
`crates/donder-language/src/compiler/definition.rs` returns the script's
`CompiledDefinition`s or its diagnostics.

1. `syntax::parse` lexes and parses the text into a `Module`.
2. `check::check` type-checks each declaration into a `Definition`: its
   parameters, inputs and dataflow IR graph (`ir`).
3. `lowering_diagnostics` lowers each definition's most general `Instance`
   (`Instance::generic`, `try_lower`) and reports one that exceeds the
   program limits.

A definition then becomes a program in three steps: `CompiledDefinition::invoke`
binds values and automation (`Invocation`), `Invocation::instance` places it
(`Instance`, `compiler/instance.rs`), and `Instance::lower`
(`compiler/lower/mod.rs`) runs `prepare`, `schedule::schedule` and
`emit::emit` to give a `Lowered` program.

## Prepare a show

`donder_elaboration::prepare(project, sequence_id, retain)` (or
`prepare_cached` with a `PreparationCache`) in
`crates/donder-elaboration/src/lib.rs` returns a `PreparedShow` that owns the
show's image; `arm` prepares a rig the same way (`rig.rs`).

1. `selection::resolve` resolves the active setup, layout and the hardware
   outputs `retain` keeps.
2. `sequence::prepare` places each clip through `stage::build`
   (`stage.rs`):
   - `Stage::new` finds the cells the retained outputs read and the nodes the
     root depends on;
   - `PreparedShow::build` runs a `ShowBuilder`: `fixture` registers geometry,
     `Clips::target` and `Clips::lower` (`Programs::clip`, which lowers each
     `Instance` once and binds it as a `BoundProgram`) give each clip its
     target and program, `ShowBuilder::clip` places it and `layer` groups a
     layer's clips;
   - `composition::prepare` builds the signal graph from layers, operators and
     the root, folding black signals and fusing operators;
   - `prepare_routes` packs the root's frame into the outputs' bytes.
3. `ShowBuilder::finish` writes the image; `PreparedShow::encode` gives its
   bytes (`.donderimage`, the Preview's content, a controller upload).

## Play it

`PreparedShow::into_playback()` gives a `ShowPlayback`
(`crates/donder-runtime/src/host/mod.rs`); `ShowPlayback::evaluate(time)`
returns the frame's fixture colors and output bytes. Without the host feature,
as on the ESP32, `Image::open(bytes)` borrows an image and
`Playback::new(image, &layout, buffer)` plays it in a caller's buffer.

1. `Playback::evaluate` (`playback.rs`) calls `run`, which finds the frame of
   `time` (`frame_at`) and returns at once when the buffers already hold it.
2. `evaluation::frames::evaluate_frame` (`evaluation.rs`) runs every node in
   order: `evaluate_layer` max-composites the layer's active clips, each
   through its `ClipEvaluator` on the strip interpreter (`dsl/vm/strip.rs`);
   `evaluate_operator` runs an operator over the plan target; a mix keeps each
   channel's maximum.
3. `patch::evaluate` packs the root's frame into the outputs' bytes
   (`OutputFrame`s).
4. `Playback::apply` applies a rig's `RigEvent`s between frames.

## Apply a GUI edit

The `apply_gui_edit` command calls `DesktopState::edit_gui_document`
(`apps/desktop/src/desktop_state/gui_editing.rs`), which returns a
`GuiEditUpdate`.

1. `mutate_gui_project_locked` checks the request's revision and clones the
   current snapshot once into a candidate session.
2. `donder_editor::apply_edit` (`crates/donder-editor/src/gui/dispatch.rs`)
   dispatches on the opened object's kind and mutates the candidate's typed
   state; model edits go through `DonderProject::apply_edits`
   (`ProjectEdit::Put`/`Remove`), whose `checked_edit` validates the result
   and admits its playback inputs.
3. `generated_source_texts` prints the affected data documents from the typed
   state.
4. `accept_gui_sources` publishes the accepted snapshot to the working copy,
   schedules saving and render refresh, and the edit is pushed to the undo
   history; `project_gui_document_change` projects the changed document for
   the frontend.
