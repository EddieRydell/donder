# Donder browser API

`donder-browser` owns an in-memory `ProjectSession`, prepares sequences with
`donder-elaboration`, and evaluates frames with `donder-runtime`. It uses
`donder-editor` for the same typed projections and mutations as the desktop.
No project files or runtime server are required.

The shared Rust contract in `donder-sequence-api` generates standalone
TypeScript types and desktop bindings through `pnpm generate:bindings`.
The desktop frontend exports `@donder/editor`: the actual timeline, inspector,
automation controls, graph editor, playback controls, and DSL source editor. A
`SequenceEditorHost` supplies commands, state, asset URLs, and host capabilities.
The desktop adapter uses Tauri; the website adapter uses this WASM session.

## Session API

- `new BrowserSession(config)` takes a `BrowserSessionConfig`: frame rate,
  duration, the measured page tree, effect/operator source documents, an
  optional website audio URL, and mark collections. These are the starting point, not history entries.
- The page tree (`BrowserPageNode`) becomes the layout: groups become layout
  groups, and each fixture becomes an inline fixture with one pixel per point.
  The website owns node ids so effect targets stay stable as the page changes.
- `render(seconds)` returns RGB bytes in depth-first page order.
- `setPageLayout(page)` replaces the page. Effects and automation rows on removed
  nodes are deleted. Page changes are not history entries; undo keeps the current page.
- `editorState()` returns the projected sequence, revision, settings, and undo state.
- `applyEdit(edit)` accepts the shared `SequenceGuiEdit` contract: effects,
  parameters, automation clips and bindings, layers, operators, graph edges,
  and marks. `applySelectionEdit(edit)` handles mixed selections and clipboard.
- `undo()` and `redo()` restore immutable project snapshots.
- `sourceDocuments()` and `setSource(path, source)` expose effect/operator DSL
  editing. A document may declare any number of effects, or any number of
  operators, but not both; the project check rejects removing a declaration the
  sequence still uses. Invalid source returns diagnostics without replacing the
  last accepted project or playback.
- `declarationSources(source)` splits a DSL source into one document per
  declaration (`<Name>.donder`, carrying the source's functions), using the
  parser's declaration spans, so a host can show or hide individual declarations.
- `replay(operations)` applies saved `BrowserOperation`s (edits, selection edits,
  and source changes) as history entries and prepares playback once, so a host
  can restore a session it saved. It stops at the first operation that no
  longer applies and reports why.
- `renderClipRaster(effectId, columns, rows)` uses the prepared runtime sampler
  and the same column timing as desktop clip rasters.

The website serves the audio file and the browser host plays it. Choosing audio
files and device output are unavailable. View state is maintained by the
browser host in memory.

## Build

```sh
cargo build -p donder-browser --target wasm32-unknown-unknown --release
wasm-bindgen target/wasm32-unknown-unknown/release/donder_browser.wasm \
  --target web --out-dir target/donder-browser/pkg
```

Generated WASM and JavaScript belong to the website build pipeline and are not
committed. In the website, run `pnpm donder:install` after submodule checkout,
then `pnpm donder:wasm` or `pnpm build`.
