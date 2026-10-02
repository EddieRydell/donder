# Donder project

This is a programmable lighting show. Help the user express their visual ideas
through effects, operators, and sequences. `project.donder` is the entry point;
its imports lead to the active show's definitions. Objects may be defined inline
or in separate documents, so directories are organizational rather than required.

Fixtures define pixel geometry, layouts place fixture instances and groups, and
patches route pixels to controller outputs. A setup connects the layout, patch,
and controllers. Sequences schedule effects, automate parameters, and compose
signals into output.

## Authoring

Most `.donder` documents are YAML. `.effect.donder` and `.operator.donder` files
use Donder's typed DSL. See `effects/` and `operators/` for working source,
including the editable standard libraries. Existing sequences, whether inline
in `project.donder` or under `sequences/`, demonstrate how definitions are used.

Effects declare typed parameters and produce colors in `color sample()`.
Operators declare signal inputs and sample them to transform or combine output.
Ordinary parameters can vary during playback where their types support
automation; `fixed param` values are resolved during preparation. Mark-driven
effects query marks and curves directly in `sample()`.

Imports are explicit and scoped to the consuming document, not inherited from
the root or caller. Paths are relative to the project root and must stay inside
it. YAML uses `from: { documents: [...] }` with an alias. Effect and operator
DSL files declare their own programs; they do not import other DSL definitions.
Downloaded resources are ordinary editable local files; there is no dependency
installation or automatic import resolution.

## CLI and validation

From the project directory:

```sh
donder --path . check
donder --help
```

Run `check` after authoring changes. It validates reachable documents, including
imports, definitions, targets, and assets; an unimported file is not covered.
A successful check does not establish how the show looks or behaves on hardware.
Use the desktop app for visual preview.

`donder --path . copy <destination>` exports loaded sources and referenced assets
to an independent project. `donder --path . init` adds workspace metadata to an
existing `project.donder`; it does not scaffold a show. If working from the Donder
source checkout, the CLI can be run there with
`cargo run -p donder-cli -- --path <project-directory> check`.
