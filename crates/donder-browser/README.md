# Donder browser API

`donder-browser` owns an in-memory `ProjectSession`, prepares sequences with
`donder-elaboration`, and evaluates frames with `donder-runtime`. It uses
`donder-editor` for the same typed projections and mutations as the desktop.
No project files or runtime server are required.

The shared Rust contract in `donder-sequence-api` generates standalone
TypeScript types and desktop bindings through `pnpm generate:bindings`.
The desktop frontend exports `@donder/editor`: the actual timeline, inspector,
automation controls, graph editor, and playback controls. A
`SequenceEditorHost` supplies commands, state, asset URLs, and host capabilities.
The desktop adapter uses Tauri; the website adapter uses this WASM session.

## Session API

- `editorState()` returns the projected sequence, revision, settings, and undo state.
- `applyEdit(edit)` accepts the shared `SequenceGuiEdit` contract: effects,
  parameters, automation clips and bindings, layers, operators, graph edges,
  and marks. `applySelectionEdit(edit)` handles mixed selections and clipboard.
- `undo()` and `redo()` restore immutable project snapshots.
- `sourceDocuments()`, `addSource(kind, source)`, and `setSource(path, source)`
  expose effect/operator DSL authoring. Invalid source returns diagnostics
  without replacing the last accepted project or playback.
- `setCharacterPositions(positions)` replaces the demo fixture geometry using
  normalized page coordinates. Position order matches `render(seconds).pixels`.
  Geometry changes do not create history entries; undo preserves current page geometry.
- `renderClipRaster(effectId, columns, rows)` uses the prepared runtime sampler
  and the same column timing as desktop clip rasters.

Browser audio-file imports and desktop device output are unavailable. View state
is maintained by the browser host in memory. The current demo uses one fixture
containing measured page characters; per-line fixtures and nested page groups
remain a separate layout feature.

## Build

```sh
cargo build -p donder-browser --target wasm32-unknown-unknown --release
wasm-bindgen target/wasm32-unknown-unknown/release/donder_browser.wasm \
  --target web --out-dir target/donder-browser/pkg
```

Generated WASM and JavaScript belong to the website build pipeline and are not
committed. In the website, run `pnpm donder:install` after submodule checkout,
then `pnpm donder:wasm` or `pnpm build`.