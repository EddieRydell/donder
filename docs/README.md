# Documentation

These pages are for people working on Donder and for anyone who wants a quick
picture of how it works. They describe current behavior; plans, journals and
superseded measurements don't belong here.

The pages a show's author needs are also published at
[donder.eddierydell.com/docs](https://donder.eddierydell.com/docs), rendered from
this directory; the website's `src/features/docs/pages.json` lists them. Links
between published pages stay on the site and other links open GitHub, so link
pages with relative paths.

**How it fits together**
- [Architecture](architecture.md): the pipeline from documents to LED frames,
  crate ownership, editing and playback.
- [Project language](project_language.md): data documents, imports, names and
  references, ownership, the schema of every object and the exact GUI
  correspondence.
- [Effect language](effect_language.md): declarations, builtins, numeric rules,
  limits and the standard effect and operator libraries.
- [Effect compiler](effect_compiler.md): source to dataflow IR, the global
  signal graph, scheduling and bytecode.
- [Output selection](output_selection.md): preparing a sequence for chosen
  controllers and ports.

**Hardware and performance**
- [ESP32 controllers](esp32_loading.md): install, upload, loading, editor
  playback, clock sync and output pins.
- [Performance](performance.md): compiler, preparation and interpreter design,
  timings and memory placement.

**Working on it**
- [Development](development.md): repository layout and the checks to run.
- [Testing and benchmarks](testing.md): the gate, where tests live, key contracts
  and Criterion workflows.
- [Firmware](../firmware/esp32/README.md): toolchain, builds, flashing and device
  measurement.

**Authoring references**
- [Create your first LED show](first_show.md): a two-pixel project from scratch.
- [Fixture authoring](fixture_authoring.md): shapes, layouts, groups and routing.
- [Live input](live_input.md): input devices, consoles and performing live.
- [Vixen effects](vixen_effects.md): the approximate Vixen ports and how they
  take a Vixen effect's settings.
